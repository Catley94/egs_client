//! Utilities: authentication, token caching, Fab library access, and file downloads.
//!
//! This module centralizes the heavy lifting used by the HTTP API layer:
//! - Auth-code login flow and cached token reuse (try_cached_login)
//! - Serialization of UserData tokens to a local file with safe permissions
//! - Convenience wrappers for EGS endpoints (account details/info, library items)
//! - Robust downloader assembling files from chunk parts described by a DownloadManifest
//!
//! Key concepts and files:
//! - Token cache (dev): ./cache/.egs_client_tokens.json (0600 on Unix)
//! - Fab cache (used by api.rs): cache/fab_list.json
//! - Download output structure: downloads/<Asset Title>/data/...
//!
//! Security note:
//! - Token file contains sensitive access/refresh tokens. Ensure your user account permissions
//!   restrict access to the file. On Unix we set 0600 automatically.
//!
//! Links:
//! - egs-api crate docs: https://docs.rs/egs-api
//! - Fab asset types: https://docs.rs/egs-api/latest/egs_api/api/types/

use std::collections::{HashMap, VecDeque};
use std::io;
use egs_api::api::types::account::{AccountData, AccountInfo, UserData};
use egs_api::api::types::fab_library::FabLibrary;
use egs_api::EpicGames;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, OnceLock};
use actix::{Actor, ActorContext, AsyncContext, StreamHandler};
use actix_web::{get, web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use dashmap::DashMap;
use egs_api::api::types::download_manifest::DownloadManifest;
use tokio::sync::broadcast;
use crate::api::{DEFAULT_CACHE_DIR_NAME, DEFAULT_DOWNLOADS_DIR_NAME};
use crate::{models, utils};

pub const EPIC_LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

/// Opens a browser to Epic login and requests the authorizationCode, then reads it from stdin.
///
/// Returns the trimmed code without quotes, suitable for EpicGames::auth_code(None, Some(code)).
///
/// Steps:
/// - Opens EPIC_LOGIN_URL in your default browser (falls back to printing URL).
/// - Prompts: "Please enter the 'authorizationCode' value from the JSON response".
/// - Reads a line from stdin, trims and removes quotes.
pub fn get_auth_code() -> String {

    if webbrowser::open(EPIC_LOGIN_URL).is_err() {
        println!("Please go to {}", EPIC_LOGIN_URL)
    }
    println!("Please enter the 'authorizationCode' value from the JSON response");

    let mut auth_code = String::new();
    let stdin = io::stdin(); // We get `Stdin` here.
    stdin.read_line(&mut auth_code).unwrap();
    auth_code = auth_code.trim().to_string();
    auth_code = auth_code.replace(|c: char| c == '"', "");
    println!("Using Auth Code: {}", auth_code);
    auth_code
}

/// Constructs a new EpicGames client instance.
///
/// The client is initially unauthenticated. Pair with try_cached_login or the
/// interactive get_auth_code + auth_code + login flow.
pub fn create_epic_games_services() -> EpicGames {
    EpicGames::new()
}

/// Fetches the current user's AccountData using the authenticated client.
///
/// Returns None if the request fails or the client is not authenticated.
pub async fn get_account_details(epic_games_services: &mut EpicGames) -> Option<AccountData> {
    // TODO What's the difference between this and get_account_info?
    epic_games_services.account_details().await
}

/// Fetches the AccountInfo for the current user ID via account_ids_details.
///
/// Note: This returns a Vec<AccountInfo> because the API supports batch lookup
/// by multiple IDs; here we pass the current account ID only.
pub async fn get_account_info(epic_games_services: &mut EpicGames) -> Option<Vec<AccountInfo>> {
    // TODO What's the difference between this and get_account_details?
    epic_games_services
        .account_ids_details(vec![epic_games_services.user_details().account_id.unwrap_or_default()])
        .await
}

// ===================== Token caching helpers =====================
/// Returns the filesystem path for the local token cache file.
///
/// Current behavior:
/// - In dev (debug builds), uses ./cache/.egs_client_tokens.json within the project directory.
/// - In release, uses XDG config: $XDG_CONFIG_HOME/egs_client/tokens.json (fallback ~/.config/egs_client/tokens.json)
///
/// Future improvements (TODO):
/// - Provide a "clear credentials" helper.
fn token_cache_path() -> PathBuf {
    // In debug builds, prefer a project-local cache file under ./cache
    if cfg!(debug_assertions) {
        return PathBuf::from("cache/.egs_client_tokens.json");
    }
    // Production/default: XDG config: $XDG_CONFIG_HOME/egs_client/tokens.json (fallback ~/.config/egs_client/tokens.json)
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    let dir = base.join("egs_client");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("Warning: failed to create config dir {}: {}", dir.display(), e);
    }
    dir.join("tokens.json")
}

/// Persists the given UserData (tokens) to the token cache file in pretty JSON.
///
/// On Unix systems, the file permissions are tightened to 0600.
pub fn save_user_details(user: &UserData) -> std::io::Result<()> {
    let path = token_cache_path();
    let data = serde_json::to_vec_pretty(user).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    fs::write(&path, data)?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }
    Ok(())
}

/// Loads UserData (tokens) from the token cache file, if it exists and parses.
pub fn load_user_details() -> Option<UserData> {
    let path = token_cache_path();
    if !path.exists() { return None; }
    let data = fs::read(path).ok()?;
    serde_json::from_slice::<UserData>(&data).ok()
}

/// Attempts to login using previously cached tokens.
///
/// Returns true if login succeeds (including when tokens are refreshed), false otherwise.
/// On success, writes back any updated expiry/refresh info to the cache file.
pub async fn try_cached_login(epic: &mut EpicGames) -> bool {
    if let Some(user) = load_user_details() {
        epic.set_user_details(user);
        if epic.login().await {
            // On successful relogin, persist any updated expiry times
            let ud = epic.user_details();
            let _ = save_user_details(&ud);
            return true;
        }
    }
    false
}

/// Retrieves the FabLibrary listing for the provided account.
///
/// This is a convenience wrapper around EpicGames::fab_library_items.
pub async fn get_fab_library_items(epic_games_services: &mut EpicGames, info: AccountData) -> Option<FabLibrary> {
    epic_games_services.fab_library_items(info.id).await
}

/// Downloads and assembles all files described in the provided DownloadManifest.
///
/// Layout:
/// - Files are written under out_root/data/<relative_path>
/// - Temporary chunk files are stored under sibling temp/ as <GUID>.chunk
///
/// Behavior highlights:
/// - Skips already present files by verifying SHA1 (when available) or total size.
/// - Downloads signed chunk URLs with a simple one-retry policy.
/// - Assembles each output file by slicing the chunk byte ranges defined in file_chunk_parts.
/// - Optionally verifies file SHA1 after assembly (when file_hash is provided).
/// - Performs atomic rename from .part to final file after successful assembly.
///
/// Returns Ok on success (including when all files are already present), or an error
/// when no files could be downloaded and none were up-to-date.
pub type ProgressFn = std::sync::Arc<dyn Fn(u32, String) + Send + Sync + 'static>;

pub async fn download_asset(dm: &DownloadManifest, _base_url: &str, out_root: &Path, progress: Option<ProgressFn>, job_id_opt: Option<&str>) -> Result<(), anyhow::Error> {
    use egs_api::api::types::chunk::Chunk;
    use sha1::{Digest, Sha1};
    use std::io::{self, Write};
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    // Concurrency controls (sane defaults; can be tuned via env)
    let max_files: usize = std::env::var("EAM_FILE_CONCURRENCY").ok().and_then(|s| s.parse().ok()).filter(|&n| n > 0).unwrap_or(2);
    let max_chunks: usize = std::env::var("EAM_CHUNK_CONCURRENCY").ok().and_then(|s| s.parse().ok()).filter(|&n| n > 0).unwrap_or(4);

    // Create base output dirs
    std::fs::create_dir_all(out_root)?;
    let temp_dir = out_root.parent().map(|p| p.join("temp")).unwrap_or_else(|| out_root.join("temp"));
    std::fs::create_dir_all(&temp_dir)?;

    // Clear any stale completion marker when starting/resuming a download
    let complete_marker = out_root.join(".download_complete");
    let _ = std::fs::remove_file(&complete_marker);

    let client = reqwest::Client::new();

    let files: Vec<_> = dm.files().into_iter().collect();
    let total_files = files.len();
    if total_files == 0 {
        return Err(anyhow::anyhow!("download manifest contains no files"));
    }

    // Early cancellation
    if is_cancelled(job_id_opt) {
        emit_event(job_id_opt, "cancelled", "Cancelled", None, None);
        return Err(anyhow::anyhow!("cancelled"));
    }

    // Setup file-level concurrency
    let file_sema = Arc::new(Semaphore::new(max_files));
    let mut join = JoinSet::new();

    #[derive(Default)]
    struct Totals { downloaded: usize, skipped_zero: usize, up_to_date: usize }
    let totals = Arc::new(tokio::sync::Mutex::new(Totals::default()));

    // Track completed files across concurrent tasks to compute overall percent
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Capture job id for async blocks
    let job_id_owned = job_id_opt.map(|s| s.to_string());

    for (file_idx, (filename, file)) in files.into_iter().enumerate() {
        if is_cancelled(job_id_opt) {
            emit_event(job_id_opt, "cancelled", "Cancelled", None, None);
            return Err(anyhow::anyhow!("cancelled"));
        }
        let permit_owner = file_sema.clone().acquire_owned().await.expect("semaphore closed");
        let client = client.clone();
        let temp_dir = temp_dir.clone();
        let out_root = out_root.to_path_buf();
        let totals = totals.clone();
        let completed = completed.clone();
        let progress = progress.clone();
        let job_id_owned = job_id_owned.clone();
        join.spawn(async move {
            let _permit = permit_owner; // hold until task end
            let file_no = file_idx + 1;
            println!("Downloading file {}/{}: {}", file_no, total_files, filename);
            io::stdout().flush().ok();

            // Prepare final output path under .../data/<filename>
            let mut out_path = out_root.clone();
            if out_path.file_name().map_or(false, |n| n == "data") == false { out_path = out_path.join("data"); }
            let out_path = out_path.join(&filename);
            if let Some(parent) = out_path.parent() { let _ = std::fs::create_dir_all(parent); }
            let tmp_out_path = out_path.with_extension("part");

            // Skip if final file already exists and matches expected hash/size
            let mut skip_existing = false;
            if out_path.exists() {
                if !file.file_hash.is_empty() {
                    if let Ok(mut f) = std::fs::File::open(&out_path) {
                        use std::io::Read;
                        let mut hasher = Sha1::new();
                        let mut buf = [0u8; 1024 * 1024];
                        loop { match f.read(&mut buf) { Ok(0) => break, Ok(n) => hasher.update(&buf[..n]), Err(_) => break } }
                        let got_hex = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();
                        if got_hex == file.file_hash { println!("  skipping: existing file is up-to-date"); skip_existing = true; }
                    }
                } else {
                    let expected_size: u64 = file.file_chunk_parts.iter().map(|p| p.size as u64).sum();
                    if let Ok(meta) = std::fs::metadata(&out_path) { if meta.len() == expected_size { println!("  skipping: existing file size matches (no hash available)"); skip_existing = true; } }
                }
            }
            if skip_existing {
                let mut t = totals.lock().await; t.up_to_date += 1;
                // Count as completed for overall percent and notify progress
                let done = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if let Some(cb) = &progress { let pct = (((done as f64) / (total_files as f64)) * 100.0).floor() as u32; (cb)(pct.min(100), format!("{} / {}", done, total_files)); }
                return Ok::<(), anyhow::Error>(());
            }

            // Ensure chunks
            let total_chunks = file.file_chunk_parts.len();
            if total_chunks == 0 {
                eprintln!("Warning: zero chunk parts listed for file {}; skipping file", filename);
                let mut t = totals.lock().await; t.skipped_zero += 1;
                // Treat as completed for overall progress and notify
                let done = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if let Some(cb) = &progress { let pct = (((done as f64) / (total_files as f64)) * 100.0).floor() as u32; (cb)(pct.min(100), format!("{} / {}", done, total_files)); }
                return Ok(());
            }

            // Per-file chunk concurrency control
            let chunk_sema = Arc::new(Semaphore::new(max_chunks));
            let mut chunk_join = JoinSet::new();

            for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
                // Early cancel before starting a chunk
                if utils::is_cancelled(job_id_owned.as_deref()) {
                    utils::emit_event(job_id_owned.as_deref(), "cancelled", "Cancelled", None, None);
                    break;
                }
                let guid = part.guid.clone();
                let link = part.link.clone();
                let client = client.clone();
                let temp_dir = temp_dir.clone();
                let job_id_inner = job_id_owned.clone();
                let chunk_permit_owner = chunk_sema.clone().acquire_owned().await.expect("chunk sema closed");
                chunk_join.spawn(async move {
                    let _p = chunk_permit_owner; // hold permit until end
                    // Cancelled? bail
                    if utils::is_cancelled(job_id_inner.as_deref()) {
                        return Err(anyhow::anyhow!("cancelled"));
                    }
                    let chunk_path = temp_dir.join(format!("{}.chunk", guid));
                    if chunk_path.exists() {
                        print!("\r  chunks: {}/{} ({}%) - using cached chunk    ", chunk_idx + 1, total_chunks, ((chunk_idx + 1) * 100 / total_chunks).min(100));
                        io::stdout().flush().ok();
                        return Ok::<(), anyhow::Error>(());
                    }
                    print!("\r  chunks: {}/{} ({}%) - downloading...        ", chunk_idx + 1, total_chunks, ((chunk_idx + 1) * 100 / total_chunks).min(100));
                    io::stdout().flush().ok();
                    let link = link.as_ref().ok_or_else(|| anyhow::anyhow!("missing signed chunk link for {}", guid))?;
                    let url = link.to_string();
                    // Check cancel right before sending
                    if utils::is_cancelled(job_id_inner.as_deref()) { return Err(anyhow::anyhow!("cancelled")); }
                    let mut resp = client.get(url.clone()).send().await;
                    if resp.is_err() { resp = client.get(url.clone()).send().await; }
                    let resp = resp.map_err(|e| anyhow::anyhow!("chunk request failed for {}: {}", guid, e))?;
                    let resp = resp.error_for_status().map_err(|e| anyhow::anyhow!("chunk HTTP {} for {}", e.status().unwrap_or_default(), guid))?;
                    // Check cancel before reading body
                    if utils::is_cancelled(job_id_inner.as_deref()) { return Err(anyhow::anyhow!("cancelled")); }
                    use futures_util::StreamExt;
                    if let Some(parent) = chunk_path.parent() { let _ = std::fs::create_dir_all(parent); }
                    let mut f = std::fs::File::create(&chunk_path)?;
                    let mut stream = resp.bytes_stream();
                    while let Some(next) = stream.next().await {
                        if utils::is_cancelled(job_id_inner.as_deref()) {
                            // Leave partial chunk; future runs may reuse/overwrite
                            return Err(anyhow::anyhow!("cancelled"));
                        }
                        let bytes = next.map_err(|e| anyhow::anyhow!("read chunk {}: {}", guid, e))?;
                        std::io::Write::write_all(&mut f, &bytes)?;
                    }
                    Ok(())
                });
            }

            // Wait all chunks; abort early on cancel
            while let Some(res) = chunk_join.join_next().await {
                if let Err(e) = res { return Err(e.into()); }
                // If a task returned Err(cancelled), propagate
                if utils::is_cancelled(job_id_owned.as_deref()) {
                    return Err(anyhow::anyhow!("cancelled"));
                }
            }
            println!("\r  chunks: {}/{} (100%) - done                    ", total_chunks, total_chunks);

            // Cancel before assembling
            if utils::is_cancelled(job_id_owned.as_deref()) {
                return Err(anyhow::anyhow!("cancelled"));
            }

            // Assemble
            let mut out = std::fs::File::create(&tmp_out_path)?;
            let mut hasher = Sha1::new();
            let total_bytes: u128 = file.file_chunk_parts.iter().map(|p| p.size as u128).sum();
            let mut written: u64 = 0;
            for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
                if utils::is_cancelled(job_id_owned.as_deref()) { return Err(anyhow::anyhow!("cancelled")); }
                let guid = &part.guid;
                let chunk_path = temp_dir.join(format!("{}.chunk", guid));
                let chunk_bytes = std::fs::read(&chunk_path)?;
                let chunk = Chunk::from_vec(chunk_bytes).ok_or_else(|| anyhow::anyhow!("failed to parse chunk {}", guid))?;
                let start = part.offset as usize;
                let end = (part.offset + part.size) as usize;
                if end > chunk.data.len() { return Err(anyhow::anyhow!("chunk too small for {} [{}..{} > {}]", filename, start, end, chunk.data.len())); }
                let slice = &chunk.data[start..end];
                std::io::Write::write_all(&mut out, slice)?;
                hasher.update(slice);
                written += part.size as u64;
                let total_chunks = file.file_chunk_parts.len();
                let mb_done = (written as f64) / (1024.0 * 1024.0);
                let mb_total = (total_bytes as f64) / (1024.0 * 1024.0);
                print!("\r  assembling: {}/{} ({}%)  [{:.2} / {:.2} MB]", chunk_idx + 1, total_chunks, ((chunk_idx + 1) * 100 / total_chunks).min(100), mb_done, mb_total);
                io::stdout().flush().ok();
            }
            println!("\r  assembling: {}/{} (100%)  [{:.2} / {:.2} MB] - done", file.file_chunk_parts.len(), file.file_chunk_parts.len(), (total_bytes as f64)/(1024.0*1024.0), (total_bytes as f64)/(1024.0*1024.0));

            if !file.file_hash.is_empty() {
                let got = hasher.finalize();
                let got_hex = got.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                if got_hex != file.file_hash { eprintln!("Warning: SHA1 mismatch for {} (expected {}, got {})", filename, file.file_hash, got_hex); }
            }

            drop(out);
            std::fs::rename(&tmp_out_path, &out_path)?;
            let mut t = totals.lock().await; t.downloaded += 1;
            // Count as completed for overall percent and notify
            let done = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if let Some(cb) = &progress { let pct = (((done as f64) / (total_files as f64)) * 100.0).floor() as u32; (cb)(pct.min(100), format!("{} / {}", done, total_files)); }
            Ok(())
        });
    }

    // Await all file tasks
    while let Some(res) = join.join_next().await {
        if let Err(e) = res { return Err(e.into()); }
        if is_cancelled(job_id_opt) {
            return Err(anyhow::anyhow!("cancelled"));
        }
    }

    let t = totals.lock().await;
    let downloaded_files = t.downloaded;
    let skipped_files = t.skipped_zero;
    let up_to_date_files = t.up_to_date;

    if downloaded_files == 0 {
        if up_to_date_files > 0 {
            eprintln!("Note: all files already present ({} up-to-date, {} with zero chunks)", up_to_date_files, skipped_files);
        } else {
            return Err(anyhow::anyhow!(format!("no files could be downloaded: {} files listed, {} skipped (zero chunks)", total_files, skipped_files)));
        }
    } else if skipped_files > 0 {
        eprintln!("Note: {} of {} files were skipped due to zero chunk parts", skipped_files, total_files);
    }

    // Mark download as complete
    let _ = std::fs::write(out_root.join(".download_complete"), "ok");
    Ok(())
}

/// Sanitize a title for use as a folder name (mirrors logic in download_asset and refresh).
pub fn sanitize_title_for_folder(s: &str) -> String {
    let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let replaced = s.replace(&illegal[..], "_");
    let trimmed = replaced.trim().trim_matches('.').to_string();
    trimmed
}


/// Annotate the provided FAB library JSON (as serde_json::Value) with `downloaded` flags
/// based on the presence of corresponding folders under downloads/.
/// Returns (total_assets, marked_downloaded, changed).
pub fn annotate_downloaded_flags(value: &mut serde_json::Value) -> (usize, usize, bool) {
    let downloads_root = default_downloads_dir();
    let mut total_assets = 0usize;
    let mut marked_downloaded = 0usize;
    let mut changed = false;

    if let Some(results) = value.get_mut("results").and_then(|v| v.as_array_mut()) {
        for asset in results.iter_mut() {
            total_assets += 1;
            let title: String = asset.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let namespace: String = asset.get("assetNamespace").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let asset_id: String = asset.get("assetId").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let mut asset_downloaded = false;
            let mut used_title_folder = false;

            if !title.is_empty() {
                let folder = utils::sanitize_title_for_folder(&title);
                let path = downloads_root.join(folder);
                if path.exists() && is_download_complete(&path) { asset_downloaded = true; used_title_folder = true; }
            }

            if !asset_downloaded {
                if let Some(versions) = asset.get_mut("projectVersions").and_then(|v| v.as_array_mut()) {
                    for ver in versions.iter_mut() {
                        let artifact_id = ver.get("artifactId").and_then(|v| v.as_str()).unwrap_or("");
                        if !namespace.is_empty() && !asset_id.is_empty() && !artifact_id.is_empty() {
                            let folder = format!("{}-{}-{}", namespace, asset_id, artifact_id);
                            let path = downloads_root.join(folder);
                            if path.exists() && is_download_complete(&path) {
                                asset_downloaded = true;
                                if let Some(obj) = ver.as_object_mut() {
                                    if obj.get("downloaded").and_then(|v| v.as_bool()) != Some(true) {
                                        obj.insert("downloaded".into(), serde_json::Value::Bool(true));
                                        changed = true;
                                    }
                                }
                                break;
                            } else {
                                if let Some(obj) = ver.as_object_mut() {
                                    if obj.get("downloaded").is_none() || obj.get("downloaded").and_then(|v| v.as_bool()).unwrap_or(true) {
                                        obj.insert("downloaded".into(), serde_json::Value::Bool(false));
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                if let Some(versions) = asset.get_mut("projectVersions").and_then(|v| v.as_array_mut()) {
                    for ver in versions.iter_mut() {
                        if let Some(obj) = ver.as_object_mut() {
                            if obj.get("downloaded").and_then(|v| v.as_bool()) != Some(true) {
                                obj.insert("downloaded".into(), serde_json::Value::Bool(true));
                                changed = true;
                            }
                        }
                    }
                }
            }

            if asset_downloaded { marked_downloaded += 1; }
            if let Some(obj) = asset.as_object_mut() {
                if obj.get("downloaded").and_then(|v| v.as_bool()) != Some(asset_downloaded) {
                    obj.insert("downloaded".into(), serde_json::Value::Bool(asset_downloaded));
                    changed = true;
                }
            }

            // If title folder was used, ensure asset-level true and versions true already handled
            if used_title_folder {
                // nothing extra
            }
        }
    }

    (total_assets, marked_downloaded, changed)
}


pub fn default_cache_dir() -> PathBuf {
    // Debug: project-local directory for easy inspection during development
    if cfg!(debug_assertions) {
        return PathBuf::from(DEFAULT_CACHE_DIR_NAME);
    }
    // Release: XDG cache: $XDG_CACHE_HOME/egs_client (fallback ~/.cache/egs_client)
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("egs_client")
}

pub fn default_downloads_dir() -> PathBuf {
    // Debug: project-local directory for easy inspection during development
    if cfg!(debug_assertions) {
        return PathBuf::from(DEFAULT_DOWNLOADS_DIR_NAME);
    }
    // Release: XDG data dir: $XDG_DATA_HOME/egs_client/downloads (fallback ~/.local/share/egs_client/downloads)
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("egs_client").join(DEFAULT_DOWNLOADS_DIR_NAME)
}

/// Checks whether a download directory contains a completion marker created after a successful download.
pub fn is_download_complete(root: &Path) -> bool {
    // Primary: explicit completion marker
    if root.join(".download_complete").is_file() { return true; }
    // Legacy heuristic: treat as complete if there are no .part files and there is at least one file under data/
    let data_dir = root.join("data");
    if !data_dir.exists() { return false; }
    let mut has_files = false;
    let mut has_part = false;
    if let Ok(iter) = walkdir::WalkDir::new(&root).into_iter().collect::<Result<Vec<_>, _>>() {
        for entry in iter {
            if entry.file_type().is_file() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("part") { has_part = true; break; }
                if p.starts_with(&data_dir) { has_files = true; }
            }
        }
    }
    if has_part { return false; }
    has_files
}

pub fn fab_cache_file() -> PathBuf {
    let dir = default_cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("fab_list.json")
}

pub fn read_build_version(engine_dir: &Path) -> Option<String> {
    // Try Engine/Build/Build.version JSON to get Major/Minor/Patch
    let build_file = engine_dir.join("Engine").join("Build").join("Build.version");
    if let Ok(bytes) = fs::read(&build_file) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            let major = v.get("MajorVersion").and_then(|x| x.as_u64()).unwrap_or(0);
            let minor = v.get("MinorVersion").and_then(|x| x.as_u64()).unwrap_or(0);
            let patch = v.get("PatchVersion").and_then(|x| x.as_u64()).unwrap_or(0);
            if major > 0 {
                if patch > 0 {
                    return Some(format!("{}.{}.{}", major, minor, patch));
                } else {
                    return Some(format!("{}.{}", major, minor));
                }
            }
        }
    }
    None
}

pub fn find_editor_binary(engine_dir: &Path) -> Option<PathBuf> {
    // Linux typical paths
    let candidates = [
        engine_dir.join("Engine/Binaries/Linux/UnrealEditor"),
        engine_dir.join("Engine/Binaries/Linux/UE4Editor"),
        engine_dir.join("Engine/Binaries/Linux/UnrealEditor.app/Contents/MacOS/UnrealEditor"), // in case of mac-like layout copied
    ];
    for c in candidates.iter() {
        if c.exists() && c.is_file() {
            return Some(c.clone());
        }
    }
    None
}

pub fn parse_version_from_name(name: &str) -> Option<String> {
    // Extract first digit-sequence like 5, 5.2, 5.2.1
    let mut version = String::new();
    let mut seen_digit = false;
    for ch in name.chars() {
        if ch.is_ascii_digit() {
            version.push(ch);
            seen_digit = true;
        } else if ch == '.' && seen_digit {
            version.push(ch);
        } else if seen_digit {
            break;
        }
    }
    if !version.is_empty() { Some(version) } else { None }
}


pub fn resolve_project_path(project_param: &str) -> Option<PathBuf> {
    let p = PathBuf::from(project_param);
    if p.is_file() {
        return Some(p);
    }
    // If directory, look for a single .uproject inside
    if p.is_dir() {
        if let Ok(entries) = fs::read_dir(&p) {
            for e in entries.flatten() {
                let fp = e.path();
                if fp.is_file() {
                    if let Some(ext) = fp.extension() { if ext == "uproject" { return Some(fp); } }
                }
            }
        }
    }
    None
}

pub fn pick_engine_for_version<'a>(engines: &'a [models::UnrealEngineInfo], requested: &str) -> Option<&'a models::UnrealEngineInfo> {
    // Try exact version match first
    if let Some(e) = engines.iter().find(|e| e.version == requested) { return Some(e); }
    // Try prefix match (e.g., request 5.3 and engine 5.3.2)
    if let Some(e) = engines.iter().find(|e| e.version.starts_with(requested)) { return Some(e); }
    // Try name contains requested (e.g., UE_5.3)
    engines.iter().find(|e| e.name.contains(requested))
}

pub fn resolve_project_dir_from_param(param: &str) -> Option<PathBuf> {
    // Reuse the existing resolver; it returns a .uproject path when found
    if let Some(p) = utils::resolve_project_path(param) {
        return p.parent().map(|p| p.to_path_buf());
    }
    // If the param is a directory, check for a .uproject inside and return the dir
    let p = PathBuf::from(param);
    if p.is_dir() {
        // Require that it looks like a UE project (contains a .uproject)
        if let Ok(entries) = fs::read_dir(&p) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().map_or(false, |ext| ext == "uproject") {
                    return Some(p);
                }
            }
        }
    }
    // As a last resort, try treating it as a project name under default projects dir
    let candidate = default_unreal_projects_dir().join(param);
    if candidate.is_dir() {
        if let Ok(entries) = fs::read_dir(&candidate) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().map_or(false, |ext| ext == "uproject") {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

pub fn copy_dir_recursive(src: &Path, dst: &Path, overwrite: bool) -> std::io::Result<(usize, usize)> {
    // Returns (copied, skipped)
    let mut copied = 0usize;
    let mut skipped = 0usize;
    if !src.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("source not found: {}", src.display())));
    }
    for entry in walkdir::WalkDir::new(src).follow_links(false) {
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if entry.file_type().is_file() {
            if target.exists() && !overwrite {
                skipped += 1;
                continue;
            }
            if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
            fs::copy(path, &target)?;
            copied += 1;
        }
    }
    Ok((copied, skipped))
}

pub fn copy_dir_recursive_with_progress(src: &Path, dst: &Path, overwrite: bool, job_id_opt: Option<&str>, phase: &str) -> std::io::Result<(usize, usize)> {
    // Returns (copied, skipped) while emitting percent progress (0..=100)
    use walkdir::WalkDir;
    if !src.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("source not found: {}", src.display())));
    }
    // Count total files
    let mut total_files: usize = 0;
    for entry in WalkDir::new(src).follow_links(false) {
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if entry.file_type().is_file() { total_files += 1; }
    }
    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut last_percent: u32 = 0;
    emit_event(job_id_opt, phase, "Starting...", Some(0.0), None);
    for entry in WalkDir::new(src).follow_links(false) {
        if is_cancelled(job_id_opt) {
            emit_event(job_id_opt, phase, "Cancelled", None, None);
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled by user"));
        }
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if entry.file_type().is_file() {
            if target.exists() && !overwrite {
                skipped += 1;
            } else {
                if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
                fs::copy(path, &target)?;
                copied += 1;
            }
            if total_files > 0 {
                let mut percent = ((copied as f64 / total_files as f64) * 100.0).floor() as u32;
                if percent > 100 { percent = 100; }
                if percent != last_percent {
                    last_percent = percent;
                    emit_event(job_id_opt, phase, format!("{} / {}", copied, total_files), Some(percent as f32), None);
                }
            }
        }
    }
    emit_event(job_id_opt, phase, "Done", Some(100.0), None);
    Ok((copied, skipped))
}

/// Ensure an asset with the given library title is available under downloads/.
/// If not present, attempts to authenticate, locate the asset in the Fab library,
/// pick one of its project_versions (latest if possible), and download it.
/// Returns the asset folder path under downloads/ on success.
pub async fn ensure_asset_downloaded_by_name(title: &str, job_id_opt: Option<&str>, phase_for_progress: &str) -> Result<PathBuf, String> {
    // Resolve downloads base similar to other endpoints
    let mut downloads_base = PathBuf::from("downloads");
    if !downloads_base.exists() {
        if let Ok(exe) = std::env::current_exe() { if let Some(exe_dir) = exe.parent() { let alt = exe_dir.join("downloads"); if alt.exists() { downloads_base = alt; } } }
    }
    // Check existing (exact/case-insensitive)
    let mut asset_dir = downloads_base.join(title);
    if !asset_dir.exists() {
        if downloads_base.is_dir() {
            if let Ok(entries) = fs::read_dir(&downloads_base) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                            if fname.eq_ignore_ascii_case(title) { asset_dir = p; break; }
                        }
                    }
                }
            }
        }
    }
    if asset_dir.exists() && is_download_complete(&asset_dir) { return Ok(asset_dir); }

    // Authenticate
    let mut epic = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic).await {
        let auth_code = utils::get_auth_code();
        let _ = epic.auth_code(None, Some(auth_code)).await;
        let _ = epic.login().await;
        let _ = utils::save_user_details(&epic.user_details());
    }

    // Load library and find asset by title (case-insensitive exact match)
    let account = utils::get_account_details(&mut epic).await.ok_or_else(|| "Unable to get account details".to_string())?;
    let library = utils::get_fab_library_items(&mut epic, account).await.ok_or_else(|| "Unable to fetch Fab library items".to_string())?;
    let asset = library.results.iter().find(|a| a.title.eq_ignore_ascii_case(title))
        .ok_or_else(|| format!("Asset '{}' not found in your Fab library", title))?;

    // Pick a project_version entry (prefer the one marked default if such field exists; else last)
    let version_opt = asset.project_versions.last();
    let version = match version_opt { Some(v) => v, None => return Err("Selected asset has no project versions to download".to_string()) };
    let artifact_id = version.artifact_id.clone();
    let namespace = asset.asset_namespace.clone();
    let asset_id = asset.asset_id.clone();

    // Fetch manifest(s) and try distribution points
    let manifest_res = epic.fab_asset_manifest(&artifact_id, &namespace, &asset_id, None).await;
    let manifests = match manifest_res { Ok(m) => m, Err(e) => return Err(format!("Failed to fetch manifest: {:?}", e)) };

    for man in manifests.iter() {
        for url in man.distribution_point_base_urls.iter() {
            if let Ok(mut dm) = epic.fab_download_manifest(man.clone(), url).await {
                // Ensure SourceURL custom field present
                use std::collections::HashMap;
                if let Some(ref mut fields) = dm.custom_fields { fields.insert("SourceURL".to_string(), url.clone()); }
                else { let mut map = HashMap::new(); map.insert("SourceURL".to_string(), url.clone()); dm.custom_fields = Some(map); }

                // Sanitize title for folder name
                let mut t = asset.title.clone();
                let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
                t = t.replace(&illegal[..], "_");
                let t = t.trim().trim_matches('.').to_string();
                let folder_name = if !t.is_empty() { t } else { format!("{}-{}-{}", namespace, asset_id, artifact_id) };
                let out_root = downloads_base.join(folder_name);
                let progress_cb: Option<utils::ProgressFn> = job_id_opt.map(|jid| {
                    let jid = jid.to_string();
                    let phase = phase_for_progress.to_string();
                    let f: utils::ProgressFn = std::sync::Arc::new(move |pct: u32, msg: String| {
                        emit_event(Some(&jid), &phase, msg.clone(), Some(pct as f32), None);
                    });
                    f
                });
                match utils::download_asset(&dm, url.as_str(), &out_root, progress_cb, job_id_opt).await {
                    Ok(_) => { return Ok(out_root); },
                    Err(e) => { eprintln!("Download failed from {}: {:?}", url, e); continue; }
                }
            }
        }
    }
    Err("Unable to download asset from any distribution point".to_string())
}





// EVENTS - WEBSOCKETS

static JOB_BUS: OnceLock<DashMap<String, broadcast::Sender<String>>> = OnceLock::new();
static JOB_BUFFER: OnceLock<DashMap<String, VecDeque<String>>> = OnceLock::new();

// Cooperative job cancellation registry
static CANCEL_MAP: OnceLock<DashMap<String, bool>> = OnceLock::new();
fn cancel_map() -> &'static DashMap<String, bool> { CANCEL_MAP.get_or_init(|| DashMap::new()) }
pub fn cancel_job(job_id: &str) { cancel_map().insert(job_id.to_string(), true); emit_event(Some(job_id), "cancel", "Cancellation requested", None, None); }
pub fn clear_cancel(job_id: &str) { let _ = cancel_map().remove(job_id); }
pub fn is_cancelled(job_id_opt: Option<&str>) -> bool { if let Some(j) = job_id_opt { cancel_map().get(j).is_some() } else { false } }

pub fn bus() -> &'static DashMap<String, broadcast::Sender<String>> {
    JOB_BUS.get_or_init(|| DashMap::new())
}

pub fn buffer_map() -> &'static DashMap<String, VecDeque<String>> {
    JOB_BUFFER.get_or_init(|| DashMap::new())
}

pub fn get_sender(job_id: &str) -> broadcast::Sender<String> {
    if let Some(s) = bus().get(job_id) { return s.clone(); }
    let (tx, _rx) = broadcast::channel::<String>(128);
    bus().insert(job_id.to_string(), tx.clone());
    tx
}

pub fn push_buffered(job_id: &str, json: String) {
    let mut entry = buffer_map().entry(job_id.to_string()).or_insert_with(|| VecDeque::with_capacity(32));
    // Keep up to 32 recent events
    if entry.len() >= 32 { entry.pop_front(); }
    entry.push_back(json);
}

pub fn take_buffer(job_id: &str) -> Vec<String> {
    if let Some(mut e) = buffer_map().get_mut(job_id) {
        let mut out = Vec::new();
        while let Some(v) = e.pop_front() { out.push(v); }
        return out;
    }
    Vec::new()
}

pub fn emit_event(job_id_opt: Option<&str>, phase: &str, message: impl Into<String>, progress: Option<f32>, details: Option<serde_json::Value>) {
    if let Some(job_id) = job_id_opt {
        let msg_str: String = message.into();
        // Debug: log every event emitted
        let pstr = match progress { Some(p) => format!("{:.1}%", p), None => "null".to_string() };
        println!("[WS][emit] job_id={} phase={} progress={} msg={}", job_id, phase, pstr, msg_str);
        let ev = models::ProgressEvent { job_id: job_id.to_string(), phase: phase.to_string(), message: msg_str, progress, details };
        if let Ok(json) = serde_json::to_string(&ev) {
            // Broadcast to current subscribers
            let _ = get_sender(job_id).send(json.clone());
            // Also buffer for late subscribers
            push_buffered(job_id, json);
        }
    }
}

// Global shutdown hook for WS-close-triggered backend stop
static SHUTDOWN_TX: OnceLock<broadcast::Sender<()>> = OnceLock::new();

pub fn set_shutdown_sender(tx: broadcast::Sender<()>) {
    let _ = SHUTDOWN_TX.set(tx);
}

pub fn request_shutdown() {
    if let Some(tx) = SHUTDOWN_TX.get() {
        let _ = tx.send(());
    }
}

fn exit_on_ws_close_enabled() -> bool {
    if let Ok(v) = std::env::var("EGS_EXIT_ON_WS_CLOSE") {
        let s = v.trim().to_ascii_lowercase();
        return s == "1" || s == "true" || s == "yes";
    }
    false
}

pub struct WsSession {
    pub rx: broadcast::Receiver<String>,
    pub job_id: String
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;
    fn stopped(&mut self, _ctx: &mut Self::Context) {
        // Do NOT shut down the backend on normal WS close.
        // Backend lifecycle is managed by process signals and (in BOTH mode) by the Flutter child watcher.
        println!("[WS] session stopped for job {}", self.job_id);
        // Previously: if exit_on_ws_close_enabled() { request_shutdown(); }
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Text(_)) => { /* ignore client messages */ },
            Ok(ws::Message::Close(_)) => {
                println!("[WS] client requested close for job {}", self.job_id);
                // Treat WebSocket close as a cancellation request for this job so
                // long-running tasks can stop cooperatively.
                cancel_job(&self.job_id);
                ctx.stop();
            },
            _ => {}
        }
    }

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("[WS] session started for job {}", self.job_id);
        // First, flush any buffered events for late subscribers
        for ev in take_buffer(&self.job_id) {
            ctx.text(ev);
        }
        // Then forward new broadcast messages to the websocket
        let mut rx = self.rx.resubscribe();
        ctx.run_interval(std::time::Duration::from_millis(500), move |act, ctx| {
            loop {
                match rx.try_recv() {
                    Ok(text) => ctx.text(text),
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => { ctx.stop(); break; }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                }
            }
        });
    }
}

pub fn config_file_path() -> PathBuf {
    // In debug builds, use local config under project cache
    if cfg!(debug_assertions) {
        let mut p = PathBuf::from(DEFAULT_CACHE_DIR_NAME);
        let _ = std::fs::create_dir_all(&p);
        p.push("config.json");
        return p;
    }
    // Production: XDG config: $XDG_CONFIG_HOME/egs_client/config.json (fallback ~/.config/egs_client/config.json)
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    let dir = base.join("egs_client");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

pub fn load_paths_config() -> models::PathsConfig {
    let path = utils::config_file_path();
    if let Ok(mut f) = std::fs::File::open(&path) {
        let mut s = String::new();
        if f.read_to_string(&mut s).is_ok() {
            if let Ok(cfg) = serde_json::from_str::<models::PathsConfig>(&s) {
                return cfg;
            }
        }
    }
    models::PathsConfig::default()
}

pub fn save_paths_config(cfg: &models::PathsConfig) -> std::io::Result<()> {
    let path = utils::config_file_path();
    let s = serde_json::to_string_pretty(cfg).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(path, s)
}

pub fn default_unreal_projects_dir() -> PathBuf {
    // 1) Config override
    if let Some(dir) = load_paths_config().projects_dir {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    // 2) Env var override
    if let Ok(val) = std::env::var("EGS_UNREAL_PROJECTS_DIR") {
        if !val.trim().is_empty() {
            return PathBuf::from(val);
        }
    }
    // 3) Default: $HOME/Documents/Unreal Projects
    if let Ok(home) = std::env::var("HOME") {
        let mut p = PathBuf::from(home);
        p.push("Documents");
        p.push("Unreal Projects");
        p
    } else {
        // Fallback to current dir if HOME not set
        PathBuf::from(".")
    }
}

pub fn default_unreal_engines_dir() -> PathBuf {
    // 1) Config override
    if let Some(dir) = utils::load_paths_config().engines_dir {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    // 2) Env var override
    if let Ok(val) = std::env::var("EGS_UNREAL_ENGINES_DIR") {
        if !val.trim().is_empty() {
            return PathBuf::from(val);
        }
    }
    // 3) Default: $HOME/UnrealEngines
    if let Ok(home) = std::env::var("HOME") {
        let mut p = PathBuf::from(home);
        p.push("UnrealEngines");
        p
    } else {
        PathBuf::from(".")
    }
}

/// Internal helper that refreshes the Fab library without initiating any downloads.
///
/// Returns a summary list (JSON) suitable for UI consumption. On auth failure or missing
/// details, returns a 200 OK with a short message body describing the condition.
pub async fn handle_refresh_fab_list() -> HttpResponse {
    // Try to use cached refresh token first (no browser, no copy-paste)
    let mut epic_games_services = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic_games_services).await {
        // No cached tokens: instruct the UI to start the interactive login flow instead of blocking on stdin.
        // Provide the URL the user must visit to obtain the authorizationCode.
        let payload = serde_json::json!({
            "unauthenticated": true,
            "auth_url": EPIC_LOGIN_URL,
            "message": "No cached credentials. Please log in via your browser and enter the authorization code in the app."
        });
        return HttpResponse::Unauthorized().json(payload);
    } else {
        println!("Logged in using cached credentials");
    }

    // Fetch account details and additional account info (for diagnostics/UI display).
    let details = utils::get_account_details(&mut epic_games_services).await;
    let info = utils::get_account_info(&mut epic_games_services).await;

    // Retrieve the Fab library based on the acquired account details.
    match details {
        None => {
            println!("No details found");
            HttpResponse::Ok().body("No details found")
        }
        Some(info) => {
            let assets = utils::get_fab_library_items(&mut epic_games_services, info).await;
            match assets {
                None => {
                    println!("No assets found");
                    HttpResponse::Ok().body("No assets found")
                }
                Some(retrieved_assets) => {
                    println!("Library items length: {:?}", retrieved_assets.results.len());

                    // Convert to JSON value so we can enrich with local-only fields like 'downloaded'.
                    let mut value = match serde_json::to_value(&retrieved_assets) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Warning: failed to convert FAB list to JSON value: {}", e);
                            return HttpResponse::Ok().json(&retrieved_assets);
                        }
                    };

                    // Compute 'downloaded' flags by checking the downloads/ directory for expected folders.
                    let downloads_root = utils::default_downloads_dir();
                    let mut total_assets = 0usize;
                    let mut marked_downloaded = 0usize;

                    if let Some(results) = value.get_mut("results").and_then(|v| v.as_array_mut()) {
                        for asset in results.iter_mut() {
                            total_assets += 1;
                            let title: String = asset.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let namespace: String = asset.get("assetNamespace").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let asset_id: String = asset.get("assetId").and_then(|v| v.as_str()).unwrap_or("").to_string();

                            let mut asset_downloaded = false;

                            // Title-based folder (preferred by downloader)
                            if !title.is_empty() {
                                let folder = utils::sanitize_title_for_folder(&title);
                                let path = downloads_root.join(folder);
                                if path.exists() { asset_downloaded = true; }
                            }

                            // Fallback: version-specific folders using namespace-assetId-artifactId
                            if !asset_downloaded {
                                if let Some(versions) = asset.get_mut("projectVersions").and_then(|v| v.as_array_mut()) {
                                    for ver in versions.iter_mut() {
                                        let artifact_id = ver.get("artifactId").and_then(|v| v.as_str()).unwrap_or("");
                                        if !namespace.is_empty() && !asset_id.is_empty() && !artifact_id.is_empty() {
                                            let folder = format!("{}-{}-{}", namespace, asset_id, artifact_id);
                                            let path = downloads_root.join(folder);
                                            if path.exists() {
                                                asset_downloaded = true;
                                                // Also annotate the version itself for finer UI, if desired.
                                                ver.as_object_mut().map(|obj| { obj.insert("downloaded".into(), serde_json::Value::Bool(true)); });
                                                break;
                                            } else {
                                                // Mark as false for explicitness (optional)
                                                ver.as_object_mut().map(|obj| { obj.insert("downloaded".into(), serde_json::Value::Bool(false)); });
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Title folder exists: mark all versions as downloaded=true as a heuristic
                                if let Some(versions) = asset.get_mut("projectVersions").and_then(|v| v.as_array_mut()) {
                                    for ver in versions.iter_mut() {
                                        ver.as_object_mut().map(|obj| { obj.insert("downloaded".into(), serde_json::Value::Bool(true)); });
                                    }
                                }
                            }

                            if asset_downloaded { marked_downloaded += 1; }
                            // Set the asset-level flag
                            asset.as_object_mut().map(|obj| { obj.insert("downloaded".into(), serde_json::Value::Bool(asset_downloaded)); });
                        }
                    }

                    println!("Annotated {} of {} assets as downloaded based on 'downloads/' folder.", marked_downloaded, total_assets);

                    // Save enriched JSON to cache for faster subsequent loads and offline-friendly UI.
                    if let Ok(json_bytes) = serde_json::to_vec_pretty(&value) {
                        let cache_path = utils::fab_cache_file();
                        if let Some(parent) = cache_path.parent() { let _ = fs::create_dir_all(parent); }
                        if let Err(e) = fs::write(&cache_path, &json_bytes) {
                            eprintln!("Warning: failed to write FAB cache: {}", e);
                        }
                    } else {
                        eprintln!("Warning: failed to serialize enriched FAB library for cache");
                    }

                    // Return enriched library items so the UI can show download indicators.
                    return HttpResponse::Ok().json(value);

                    // Reached only if json() above wasn't returned; keep OK fallback
                    HttpResponse::Ok().finish()
                }
            }
        }
    }
}
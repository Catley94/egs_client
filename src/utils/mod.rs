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
use egs_api::api::types::account::{AccountData, UserData};
use egs_api::api::types::fab_library::FabLibrary;
use egs_api::EpicGames;
use serde_json;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use actix::{Actor, ActorContext, AsyncContext, StreamHandler};
use actix_web::{web, HttpResponse};
use actix_web::web::Query;
use actix_web_actors::ws;
use dashmap::DashMap;
use egs_api::api::types::download_manifest::DownloadManifest;
use tokio::sync::broadcast;
use crate::api::{DEFAULT_CACHE_DIR_NAME, DEFAULT_DOWNLOADS_DIR_NAME};
use crate::{models, utils};
use crate::models::Phase;

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

pub async fn download_asset(dm: &DownloadManifest, _base_url: &str, download_directory_full_path: &Path, progress_callback: Option<ProgressFn>, job_id_opt: Option<&str>) -> Result<(), anyhow::Error> {
    use egs_api::api::types::chunk::Chunk;
    use sha1::{Digest, Sha1};
    use std::io::{self, Write};
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;
    use std::time::{Instant, Duration};

    // Concurrency controls (sane defaults; can be tuned via env)
    let max_files: usize = std::env::var("EAM_FILE_CONCURRENCY").ok().and_then(|s| s.parse().ok()).filter(|&n| n > 0).unwrap_or(2);
    let max_chunks: usize = std::env::var("EAM_CHUNK_CONCURRENCY").ok().and_then(|s| s.parse().ok()).filter(|&n| n > 0).unwrap_or(4);

    // Create asset folder
    std::fs::create_dir_all(download_directory_full_path)?;
    // Create temp folder under each asset for chunk downloads
    let temp_dir = download_directory_full_path.parent().map(|p| p.join("temp")).unwrap_or_else(|| download_directory_full_path.join("temp"));
    std::fs::create_dir_all(&temp_dir)?;

    // Clear any stale completion marker when starting/resuming a download
    let complete_marker = download_directory_full_path.join(".download_complete");
    match std::fs::remove_file(&complete_marker) {
        Ok(_) => {
            println!("Clearing stale completion marker: {}", complete_marker.display());
        }
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(anyhow::anyhow!("Failed to clear stale completion marker: {}", e));
            }
        }
    }

    let client = reqwest::Client::new();

    // Get list of files to download
    let files: Vec<_> = dm.files().into_iter().collect();
    let total_files = files.len();
    if total_files == 0 {
        return Err(anyhow::anyhow!("download manifest contains no files"));
    }

    // Precompute total bytes across all files and a shared bytes_done counter for live speed
    let total_bytes_all: u64 = files.iter()
        .map(|(_, f)| f.file_chunk_parts.iter().map(|p| p.size as u64).sum::<u64>())
        .sum();

    let bytes_done = Arc::new(AtomicU64::new(0));

    // Check if job has been requested to cancel
    if check_if_job_is_cancelled(job_id_opt) {
        cancel_this_job(job_id_opt);
        return Err(anyhow::anyhow!("cancelled"));
    }


    // Setup file-level concurrency
    let file_sema = Arc::new(Semaphore::new(max_files));
    let mut join = JoinSet::new();

    let totals = Arc::new(tokio::sync::Mutex::new(models::Totals::default()));

    // Track completed files across concurrent tasks to compute overall percent
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Capture job id for async blocks
    let job_id_owned = job_id_opt.map(|s| s.to_string());

    for (file_index, (filename, file)) in files.into_iter().enumerate() {
        // Check if job has been requested to cancel
        if check_if_job_is_cancelled(job_id_opt) {
            cancel_this_job(job_id_opt);
            return Err(anyhow::anyhow!("cancelled"));
        }

        let permit_owner = file_sema.clone().acquire_owned().await.expect("semaphore closed");

        let client = client.clone();
        let temp_dir = temp_dir.clone();
        let out_directory = download_directory_full_path.to_path_buf();
        let totals = totals.clone();
        let completed = completed.clone();
        let progress = progress_callback.clone();
        let job_id_owned = job_id_owned.clone();
        let bytes_done = bytes_done.clone();
        let _total_bytes_all = total_bytes_all;

        join.spawn(async move {
            let _permit = permit_owner; // hold until task end
            let file_no = file_index + 1;
            println!("Downloading file {}/{}: {}", file_no, total_files, filename);
            io::stdout().flush().ok();
            // Total bytes for this file (sum of chunk parts)
            let file_total_bytes: u64 = file.file_chunk_parts.iter().map(|p| p.size as u64).sum();

            // Prepare final output path under .../data/<filename>
            let mut out_path = out_directory.clone();
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
                // Count these bytes toward total progress
                let cur = bytes_done.fetch_add(file_total_bytes, Ordering::SeqCst) + file_total_bytes;
                let mut totals_locked = totals.lock().await; totals_locked.up_to_date += 1;

                // Count as completed for overall percent and notify progress
                let done = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if let Some(cb) = &progress { let pct = (((done as f64) / (total_files as f64)) * 100.0).floor() as u32; (cb)(pct.min(100), format!("{} / {}", done, total_files)); }
                // Also emit a detailed progress event so UI can show bytes
                utils::emit_event(
                    job_id_owned.as_deref(),
                    models::Phase::DownloadProgress,
                    format!("{} / {}", done, total_files),
                    Some(((done as f64) / (total_files as f64) * 100.0) as f32),
                    Some(serde_json::json!({
                        "downloaded_files": done,
                        "total_files": total_files,
                        "bytes_done": cur,
                        "total_bytes": _total_bytes_all,
                    })),
                );
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
                // Emit a detailed progress event even for zero-chunk files
                utils::emit_event(
                    job_id_owned.as_deref(),
                    models::Phase::DownloadProgress,
                    format!("{} / {}", done, total_files),
                    Some(((done as f64) / (total_files as f64) * 100.0) as f32),
                    Some(serde_json::json!({
                        "downloaded_files": done,
                        "total_files": total_files,
                        "bytes_done": bytes_done.load(std::sync::atomic::Ordering::SeqCst),
                        "total_bytes": _total_bytes_all,
                    })),
                );
                return Ok(());
            }

            // Per-file chunk concurrency control
            let chunk_sema = Arc::new(Semaphore::new(max_chunks));
            let mut chunk_join = JoinSet::new();

            for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
                // Check if job has been requested to be cancelled
                if utils::check_if_job_is_cancelled(job_id_owned.as_deref()) {
                    utils::emit_event(job_id_owned.as_deref(), models::Phase::Cancelled, "Cancelled", None, None);
                    break;
                }
                let guid = part.guid.clone();
                let link = part.link.clone();
                let client = client.clone();
                let temp_dir = temp_dir.clone();
                let job_id_inner = job_id_owned.clone();
                let chunk_permit_owner = chunk_sema.clone().acquire_owned().await.expect("chunk sema closed");
                let completed = completed.clone();
                let bytes_done = bytes_done.clone();
                chunk_join.spawn(async move {
                    let _p = chunk_permit_owner; // hold permit until end
                    // Cancelled? bail
                    if utils::check_if_job_is_cancelled(job_id_inner.as_deref()) {
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
                    if utils::check_if_job_is_cancelled(job_id_inner.as_deref()) {
                        return Err(anyhow::anyhow!("cancelled"));
                    }
                    let mut resp = client.get(url.clone()).send().await;
                    if resp.is_err() {
                        resp = client.get(url.clone()).send().await;

                    }
                    let resp = resp.map_err(|e| anyhow::anyhow!("chunk request failed for {}: {}", guid, e))?;
                    let resp = resp.error_for_status().map_err(|e| anyhow::anyhow!("chunk HTTP {} for {}", e.status().unwrap_or_default(), guid))?;

                    // Check cancel before reading body
                    if utils::check_if_job_is_cancelled(job_id_inner.as_deref()) {
                        return Err(anyhow::anyhow!("cancelled"));
                    }

                    use futures_util::StreamExt;

                    if let Some(parent) = chunk_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    let mut _file = std::fs::File::create(&chunk_path)?;

                    let mut stream = resp.bytes_stream();
                    let mut last_emit = Instant::now();
                    while let Some(next) = stream.next().await {
                        if utils::check_if_job_is_cancelled(job_id_inner.as_deref()) {
                            // Leave partial chunk; future runs may reuse/overwrite
                            return Err(anyhow::anyhow!("cancelled"));
                        }

                        let bytes = next.map_err(|e| anyhow::anyhow!("read chunk {}: {}", guid, e))?;
                        std::io::Write::write_all(&mut _file, &bytes)?;

                        // Update global bytes_done and emit throttled progress for live speed in UI
                        let cur = bytes_done.fetch_add(bytes.len() as u64, Ordering::SeqCst) + (bytes.len() as u64);
                        if last_emit.elapsed() >= Duration::from_millis(300) {
                            let done_files = completed.load(std::sync::atomic::Ordering::SeqCst);
                            let _percentage = if _total_bytes_all > 0 { ((cur as f64) / (_total_bytes_all as f64) * 100.0) as f32 } else { 0.0 };

                            utils::emit_event(
                                job_id_inner.as_deref(),
                                models::Phase::DownloadProgress,
                                format!("{} / {}", done_files, total_files),
                                Some(_percentage),
                                Some(serde_json::json!({
                                    "downloaded_files": done_files,
                                    "total_files": total_files,
                                    "bytes_done": cur,
                                    "total_bytes": _total_bytes_all,
                                })),
                            );
                            last_emit = Instant::now();
                        }
                    }
                    Ok(())
                });
            }

            // Wait all chunks; abort early on cancel
            while let Some(res) = chunk_join.join_next().await {
                if let Err(e) = res { return Err(e.into()); }
                // If a task returned Err(cancelled), propagate
                if utils::check_if_job_is_cancelled(job_id_owned.as_deref()) {
                    return Err(anyhow::anyhow!("cancelled"));
                }
            }
            println!("\r  chunks: {}/{} (100%) - done                    ", total_chunks, total_chunks);

            // Cancel before assembling
            if utils::check_if_job_is_cancelled(job_id_owned.as_deref()) {
                return Err(anyhow::anyhow!("cancelled"));
            }

            // Assemble
            let mut out = std::fs::File::create(&tmp_out_path)?;
            let mut hasher = Sha1::new();
            let total_bytes: u128 = file.file_chunk_parts.iter().map(|p| p.size as u128).sum();
            let mut written: u64 = 0;
            for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
                if utils::check_if_job_is_cancelled(job_id_owned.as_deref()) { return Err(anyhow::anyhow!("cancelled")); }
                let guid = &part.guid;
                let chunk_path = temp_dir.join(format!("{}.chunk", guid));
                let chunk_bytes = std::fs::read(&chunk_path)?;
                // Some distribution links (e.g., certain FAB endpoints) may return raw byte blobs rather than
                // Epic chunk container files. Try to parse as a chunk first; if that fails, fall back to raw bytes.
                let (data, data_len): (std::borrow::Cow<[u8]>, usize) = if let Some(chunk) = Chunk::from_vec(chunk_bytes.clone()) {
                    let len = chunk.data.len();
                    (std::borrow::Cow::Owned(chunk.data), len)
                } else {
                    let len = chunk_path.metadata().map(|m| m.len() as usize).unwrap_or(0);
                    (std::borrow::Cow::Owned(chunk_bytes), len)
                };
                let start = part.offset as usize;
                let end = (part.offset + part.size) as usize;
                if end > data_len { return Err(anyhow::anyhow!("chunk/raw too small for {} [{}..{} > {}]", filename, start, end, data_len)); }
                let slice = &data[start..end];
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
            // Emit a detailed progress event on file completion as well
            utils::emit_event(
                job_id_owned.as_deref(),
                models::Phase::DownloadProgress,
                format!("{} / {}", done, total_files),
                Some(((done as f64) / (total_files as f64) * 100.0) as f32),
                Some(serde_json::json!({
                    "downloaded_files": done,
                    "total_files": total_files,
                    "bytes_done": bytes_done.load(std::sync::atomic::Ordering::SeqCst),
                    "total_bytes": _total_bytes_all,
                })),
            );
            Ok(())
        });
    }

    // Await all file tasks
    while let Some(res) = join.join_next().await {
        if let Err(e) = res { return Err(e.into()); }
        if check_if_job_is_cancelled(job_id_opt) {
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
    let _ = std::fs::write(download_directory_full_path.join(".download_complete"), "ok");

    // After a successful download, remove the temporary chunks folder under the asset
    // The temp directory is created relative to the asset root (e.g., downloads/<Asset>/temp),
    // so compute it the same way we did earlier.
    let temp_dir_final = download_directory_full_path.parent().map(|p| p.join("temp")).unwrap_or_else(|| download_directory_full_path.join("temp"));
    match std::fs::remove_dir_all(&temp_dir_final) {
        Ok(_) => {
            println!("Cleaned up temp folder: {}", temp_dir_final.display());
        }
        Err(e) => {
            // Ignore when it does not exist; warn on other errors
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Warning: failed to remove temp folder {}: {}", temp_dir_final.display(), e);
            }
        }
    }

    Ok(())
}

fn cancel_this_job(job_id_opt: Option<&str>) {
    emit_event(job_id_opt, models::Phase::Cancelled, "Job Cancelled", None, None);
    if let Some(ref j) = job_id_opt { acknowledge_cancel(j); }
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
    let downloads_root = get_default_downloads_dir_path();
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
            let mut version_folders: Vec<String> = Vec::new();

            if !title.is_empty() {
                let folder = utils::sanitize_title_for_folder(&title);
                let path = downloads_root.join(&folder);
                if path.exists() {
                    // Legacy: direct download into title folder
                    if is_download_complete(&path) { asset_downloaded = true; used_title_folder = true; }
                    // New: versioned subfolders under title
                    if let Ok(entries) = fs::read_dir(&path) {
                        for e in entries.flatten() {
                            let p = e.path();
                            if p.is_dir() {
                                // folder name should be UE major.minor like 5.6 or 4.27
                                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                                    let mm = name.trim();
                                    if !mm.is_empty() && is_download_complete(&p) {
                                        version_folders.push(mm.to_string());
                                        asset_downloaded = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Annotate per-version flags based ONLY on versioned title subfolders to avoid over-marking.
            if let Some(versions) = asset.get_mut("projectVersions").and_then(|v| v.as_array_mut()) {
                for ver in versions.iter_mut() {
                    let mut ver_downloaded = false;
                    if !version_folders.is_empty() {
                        if let Some(ev) = ver.get("engineVersions").and_then(|v| v.as_array()) {
                            'outer: for mm in version_folders.iter() {
                                let token = format!("UE_{}", mm);
                                for e in ev.iter() {
                                    if e.as_str().map_or(false, |s| s.trim() == token) {
                                        ver_downloaded = true; break 'outer;
                                    }
                                }
                            }
                        }
                    }
                    if let Some(obj) = ver.as_object_mut() {
                        let prev = obj.get("downloaded").and_then(|v| v.as_bool());
                        if prev != Some(ver_downloaded) {
                            obj.insert("downloaded".into(), serde_json::Value::Bool(ver_downloaded));
                            changed = true;
                        }
                    }
                }
            }

            // Record the exact downloaded UE versions at the asset root for precise UI logic
            if let Some(obj) = asset.as_object_mut() {
                // Set asset-level downloaded flag
                if obj.get("downloaded").and_then(|v| v.as_bool()) != Some(asset_downloaded) {
                    obj.insert("downloaded".into(), serde_json::Value::Bool(asset_downloaded));
                    changed = true;
                }
                // Inject/update downloadedVersions array (sorted unique)
                let mut versions_unique = version_folders.clone();
                versions_unique.sort();
                versions_unique.dedup();
                let new_val = serde_json::Value::Array(versions_unique.into_iter().map(serde_json::Value::String).collect());
                let prev = obj.get("downloadedVersions").cloned();
                if prev.as_ref() != Some(&new_val) {
                    obj.insert("downloadedVersions".into(), new_val);
                    changed = true;
                }
            }

            if asset_downloaded { marked_downloaded += 1; }

            // No blanket marking of versions when using title folder; handled above via subfolders
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

pub fn get_default_downloads_dir_path() -> PathBuf {
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
    // Only trust the explicit completion marker to avoid false positives after cancellations.
    root.join(".download_complete").is_file()
}

pub fn get_fab_cache_file_path() -> PathBuf {
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

pub fn normalize_engine_association(assoc: &str) -> Option<String> {
    let mut s = assoc.trim();
    if s.is_empty() { return None; }
    if let Some(rest) = s.strip_prefix("UE_") { s = rest; }
    // Accept patterns like 5, 5.4, 5.4.1
    let mut parts = s.split('.');
    let major = parts.next().unwrap_or("");
    if major.chars().all(|c| c.is_ascii_digit()) && !major.is_empty() {
        let minor = parts.next().unwrap_or("0");
        if minor.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{}.{}", major, minor));
        }
    }
    None
}

/// Read BuildId from Engine/Build/Build.version if present
pub fn read_build_id(engine_dir: &Path) -> Option<String> {
    let build_file = engine_dir.join("Engine").join("Build").join("Build.version");
    if let Ok(bytes) = fs::read(&build_file) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(id) = v.get("BuildId").and_then(|x| x.as_str()) {
                let s = id.trim();
                if !s.is_empty() { return Some(s.to_string()); }
            }
        }
    }
    None
}

/// Convert version like "5.6.1" or "5.6" to major.minor form, e.g., "5.6"
pub fn to_major_minor(ver: &str) -> String {
    let mut it = ver.split('.');
    let major = it.next().unwrap_or("");
    let minor = it.next().unwrap_or("0");
    if !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()) && minor.chars().all(|c| c.is_ascii_digit()) {
        format!("{}.{}", major, minor)
    } else {
        ver.to_string()
    }
}

/// Resolve EngineAssociation to UE major.minor. Handles numeric strings and GUID BuildIds.
pub fn resolve_engine_association_to_mm(assoc: &str) -> Option<String> {
    if let Some(mm) = normalize_engine_association(assoc) {
        return Some(mm);
    }
    let s = assoc.trim();
    if s.is_empty() { return None; }
    // Detect GUID-like: 8-4-4-4-12 hex groups
    let is_guid_like = {
        let parts: Vec<&str> = s.split('-').collect();
        parts.len() == 5 && parts[0].len() == 8 && parts[1].len() == 4 && parts[2].len() == 4 && parts[3].len() == 4 && parts[4].len() == 12 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
    };
    if !is_guid_like { return None; }

    let engines_root = default_unreal_engines_dir();
    if let Ok(entries) = fs::read_dir(&engines_root) {
        for ent in entries.flatten() {
            let dir = ent.path();
            if dir.is_dir() {
                // Require it looks like an engine dir containing Engine/Build/Build.version
                let build_file = dir.join("Engine").join("Build").join("Build.version");
                if build_file.is_file() {
                    if let Some(build_id) = read_build_id(&dir) {
                        if build_id.eq_ignore_ascii_case(s) {
                            if let Some(ver) = read_build_version(&dir) {
                                return Some(to_major_minor(&ver));
                            }
                        }
                    }
                }
            }
        }
    }
    None
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

pub fn copy_dir_recursive_with_progress(src: &Path, dst: &Path, overwrite: bool, job_id_opt: Option<&str>, phase: models::Phase) -> std::io::Result<(usize, usize)> {
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
        if check_if_job_is_cancelled(job_id_opt) {
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
pub async fn ensure_asset_downloaded_by_name(title: &str, job_id_opt: Option<&str>, phase_for_progress: models::Phase) -> Result<PathBuf, String> {
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
                    let phase = phase_for_progress;
                    let f: utils::ProgressFn = std::sync::Arc::new(move |pct: u32, msg: String| {
                        emit_event(Some(&jid), phase, msg.clone(), Some(pct as f32), None);
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
pub fn cancel_job(job_id: &str) { cancel_map().insert(job_id.to_string(), true); emit_event(Some(job_id), models::Phase::Cancel, "Cancellation requested", None, None); }
pub fn acknowledge_cancel(job_id: &str) { let _ = cancel_map().remove(job_id); }
pub fn check_if_job_is_cancelled(job_id_opt: Option<&str>) -> bool { if let Some(j) = job_id_opt { cancel_map().get(j).is_some() } else { false } }

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

pub fn emit_event(job_id_opt: Option<&str>, phase: Phase, message: impl Into<String>, progress: Option<f32>, details: Option<serde_json::Value>) {
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
                println!("[WS] client closed WS for job {} (not treating as cancellation)", self.job_id);
                // Do not auto-cancel on WS close; user must hit Cancel or call /cancel-job explicitly.
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

                    // Compute 'downloaded' flags (asset-level and per-version) using filesystem state.
                    let (_total_assets, _marked, _changed) = annotate_downloaded_flags(&mut value);

                    // Save enriched JSON to cache for faster subsequent loads and offline-friendly UI.
                    if let Ok(json_bytes) = serde_json::to_vec_pretty(&value) {
                        let cache_path = utils::get_fab_cache_file_path();
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

pub fn update_fab_cache_json(namespace: String, asset_id: String, artifact_id: String, ue_major_minor_version: Option<String>, title_folder: Option<String>, cache_path: &PathBuf) {
    if let Ok(mut f) = fs::File::open(&cache_path) {
        use std::io::Read as _;
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_ok() {
            if let Ok(mut cache_val) = serde_json::from_slice::<serde_json::Value>(&buf) {
                let mut changed = false;
                let mut found_asset = false;
                let mut found_version = false;
                if let Some(results) = cache_val.get_mut("results").and_then(|v| v.as_array_mut()) {
                    for asset_obj in results.iter_mut() {
                        let a_ns = asset_obj.get("assetNamespace").and_then(|v| v.as_str()).unwrap_or("");
                        let a_id = asset_obj.get("assetId").and_then(|v| v.as_str()).unwrap_or("");
                        if a_ns == namespace && a_id == asset_id {
                            found_asset = true;
                            if let Some(obj) = asset_obj.as_object_mut() {
                                // Ensure asset-level flag is true
                                if obj.get("downloaded").and_then(|v| v.as_bool()) != Some(true) {
                                    obj.insert("downloaded".into(), serde_json::Value::Bool(true));
                                    changed = true;
                                }
                                // Append to downloadedVersions array if ue major.minor known
                                if let Some(ref mm) = ue_major_minor_version {
                                    let dv = obj.entry("downloadedVersions").or_insert(serde_json::Value::Array(Vec::new()));
                                    if let serde_json::Value::Array(arr) = dv {
                                        if !arr.iter().any(|v| v.as_str() == Some(mm)) {
                                            arr.push(serde_json::Value::String(mm.clone()));
                                            changed = true;
                                        }
                                    }
                                }
                            }
                            if let Some(vers) = asset_obj.get_mut("projectVersions").and_then(|v| v.as_array_mut()) {
                                for ver in vers.iter_mut() {
                                    let art = ver.get("artifactId").and_then(|v| v.as_str()).unwrap_or("");
                                    let mut should_mark = false;
                                    if art == artifact_id {
                                        should_mark = true;
                                        found_version = true;
                                    }
                                    if !should_mark {
                                        if let Some(ref mm) = ue_major_minor_version {
                                            // Mark any version that supports the selected UE major.minor
                                            if let Some(ea) = ver.get("engineVersions").and_then(|v| v.as_array()) {
                                                let token = format!("UE_{}", mm);
                                                if ea.iter().any(|e| e.as_str().map_or(false, |s| s.trim() == token)) {
                                                    should_mark = true;
                                                }
                                            }
                                        }
                                    }
                                    if should_mark {
                                        if let Some(vobj) = ver.as_object_mut() {
                                            if vobj.get("downloaded").and_then(|v| v.as_bool()) != Some(true) {
                                                vobj.insert("downloaded".into(), serde_json::Value::Bool(true));
                                                changed = true;
                                            }
                                        }
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
                if !found_asset {
                    eprintln!("Note: downloaded asset not found in cached FAB list (ns={}, id={}). Cache not updated.", namespace, asset_id);
                } else if !found_version && title_folder.is_none() {
                    eprintln!("Note: matching version (artifact {}) not found under asset {}. Only asset-level flag may be updated.", artifact_id, asset_id);
                }
                if changed {
                    if let Ok(bytes) = serde_json::to_vec_pretty(&cache_val) {
                        if let Err(e) = fs::write(&cache_path, &bytes) {
                            eprintln!("Warning: failed to update FAB cache after download: {}", e);
                        } else {
                            println!("Updated FAB cache to mark asset {} / {} (artifact {}) as downloaded.", namespace, asset_id, artifact_id);
                        }
                    }
                }
            } else {
                eprintln!("Warning: failed to parse existing FAB cache for update");
            }
        } else {
            eprintln!("Warning: failed to read existing FAB cache for update");
        }
    } else {
        eprintln!("Info: FAB cache file not found at {}. Skipping cache update.", cache_path.display());
    }
}

pub fn get_friendly_folder_name(asset_name: String) -> Option<String> {
    // Resolve a human-friendly title for folder name, if available.
    let mut title_folder: Option<String> = None;
    let mut t = asset_name.clone();
    // Replace characters illegal on common filesystems.
    let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    t = t.replace(&illegal[..], "_");
    // Also trim leading/trailing spaces and dots (Windows quirk).
    let t = t.trim().trim_matches('.').to_string();
    if !t.is_empty() {
        title_folder = Some(t);
    }
    title_folder
}

pub async fn get_friendly_asset_name(namespace: &String, asset_id: &String, artifact_id: &String, mut epic_services: &mut EpicGames) -> String {
    let mut display_name = format!("{}/{}/{}", namespace, asset_id, artifact_id);
    if let Some(details) = utils::get_account_details(&mut epic_services).await {
        if let Some(lib) = utils::get_fab_library_items(&mut epic_services, details).await {
            // Loop through Fab Library items in account and match namespace and asset ID
            if let Some(asset) = lib.results.iter().find(|a| a.asset_namespace == *namespace && a.asset_id == *asset_id) {
                if asset.project_versions.iter().any(|v| v.artifact_id == *artifact_id) {
                    let t = asset.title.trim();
                    if !t.is_empty() {
                        display_name = t.to_string();
                    }
                }
            }
        }
    }
    display_name
}

pub async fn epic_authenticate(epic_services: &mut EpicGames) {
    let auth_code = utils::get_auth_code();
    let _ = epic_services.auth_code(None, Some(auth_code)).await;
    let _ = epic_services.login().await;
    let _ = utils::save_user_details(&epic_services.user_details());
}

pub async fn handle_fab_download(
    req: &models::CreateUnrealProjectRequest,
    job_id: &Option<String>,
) -> Option<HttpResponse> {
    let (namespace, asset_id, artifact_id) = match (&req.namespace, &req.asset_id, &req.artifact_id) {
        (Some(ns), Some(aid), Some(arid)) => (ns.clone(), aid.clone(), arid.clone()),
        _ => return None,
    };

    let mut q: HashMap<String, String> = HashMap::new();
    if let Some(ref j) = job_id {
        q.insert("jobId".to_string(), j.clone());
    }
    if let Some(ref ue) = req.ue {
        if !ue.trim().is_empty() {
            q.insert("ue".to_string(), ue.trim().to_string());
        }
    }

    let path = web::Path::from((namespace, asset_id, artifact_id));
    let query = web::Query(q);

    match download_asset_handler(path, query).await {
        Err(err_response) => {
            if !err_response.status().is_success() {
                return Some(err_response);
            }
            if utils::check_if_job_is_cancelled(job_id.as_deref()) {
                if let Some(ref j) = job_id {
                    utils::acknowledge_cancel(j);
                }
                return Some(HttpResponse::Ok().body("cancelled"));
            }
        }
        Ok(response) => return Some(response),
    }

    None
}

pub fn validate_request(req: &models::CreateUnrealProjectRequest) -> Result<(), HttpResponse> {
    let template_empty = req.template_project.as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    let asset_empty = req.asset_name.as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);

    if template_empty && asset_empty {
        return Err(HttpResponse::BadRequest().body(
            "Provide either template_project (path/dir) or asset_name (under downloads/)"
        ));
    }
    if req.output_dir.trim().is_empty() {
        return Err(HttpResponse::BadRequest().body("output_dir is required"));
    }
    if req.project_name.trim().is_empty() {
        return Err(HttpResponse::BadRequest().body("project_name is required"));
    }

    let project_type = req.project_type.as_deref().unwrap_or("bp").to_lowercase();
    if project_type != "bp" && project_type != "cpp" {
        return Err(HttpResponse::BadRequest().body("project_type must be 'bp' or 'cpp'"));
    }

    Ok(())
}

pub fn resolve_engine_path(req: &models::CreateUnrealProjectRequest) -> Result<PathBuf, HttpResponse> {
    // If explicit engine_path provided, use it
    if let Some(p) = &req.engine_path {
        return Ok(PathBuf::from(p));
    }

    let base = utils::default_unreal_engines_dir();

    // If UE version specified, find matching engine
    if let Some(ue) = &req.ue {
        let engines = discover_engines(&base);
        return match utils::pick_engine_for_version(&engines, ue) {
            Some(info) => Ok(PathBuf::from(info.path.clone())),
            None => Err(HttpResponse::NotFound().body(
                "Requested UE version not found among discovered engines"
            )),
        };
    }

    // Otherwise, pick latest engine
    select_latest_engine(&base)
}

pub fn discover_engines(base: &Path) -> Vec<models::UnrealEngineInfo> {
    let mut engines = Vec::new();
    if !base.is_dir() {
        return engines;
    }

    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() || !p.join("Engine").join("Binaries").exists() {
                continue;
            }

            let name = p.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let version = utils::read_build_version(&p)
                .or_else(|| utils::parse_version_from_name(&name))
                .unwrap_or_else(|| "unknown".to_string());
            let editor_path = utils::find_editor_binary(&p)
                .map(|pp| pp.to_string_lossy().to_string());

            engines.push(models::UnrealEngineInfo {
                name,
                version,
                path: p.to_string_lossy().to_string(),
                editor_path,
            });
        }
    }
    engines
}

pub fn select_latest_engine(base: &Path) -> Result<PathBuf, HttpResponse> {
    if !base.is_dir() {
        return Err(HttpResponse::BadRequest().body(
            "engine_path not provided and no engines found in default location"
        ));
    }

    let mut engines: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("Engine").exists() {
                engines.push(p);
            }
        }
    }

    if engines.is_empty() {
        return Err(HttpResponse::BadRequest().body(
            "engine_path not provided and no engines found in default location"
        ));
    }

    engines.sort_by(|a, b| {
        b.file_name()
            .unwrap_or_default()
            .cmp(a.file_name().unwrap_or_default())
    });

    Ok(engines[0].clone())
}

pub async fn resolve_template_path(
    req: &models::CreateUnrealProjectRequest,
    job_id: &Option<String>,
) -> Result<PathBuf, HttpResponse> {
    let template_path = if let Some(tp) = &req.template_project {
        resolve_from_template_project(tp)?
    } else if let Some(name) = &req.asset_name {
        resolve_from_asset_name(name, req, job_id).await?
    } else {
        return Err(HttpResponse::BadRequest().body("No template source provided"));
    };

    match template_path {
        Some(p) if p.extension().and_then(|s| s.to_str()) == Some("uproject") => {
            println!("Using template .uproject: {}", p.to_string_lossy());

            // Canonicalize to absolute path
            Ok(std::fs::canonicalize(&p).unwrap_or_else(|_| {
                std::env::current_dir()
                    .map(|cwd| cwd.join(&p))
                    .unwrap_or(p)
            }))
        }
        _ => Err(HttpResponse::BadRequest().body(
            "Unable to resolve a .uproject from template_project/asset_name. \
             Tips: ensure there is a .uproject inside the selected folder; if using asset_name, \
             verify the asset exists under downloads/ (case-insensitive match is supported) \
             and that the .uproject isn't packaged deep inside nested 'data' or 'Content' folders."
        )),
    }
}

pub fn resolve_from_template_project(tp: &str) -> Result<Option<PathBuf>, HttpResponse> {
    let tp = tp.trim();
    if tp.is_empty() {
        return Ok(None);
    }

    let candidate = PathBuf::from(trim_quotes_and_expand_home(tp));
    Ok(if candidate.is_dir() {
        find_uproject_bfs(&candidate, 5)
    } else {
        Some(candidate)
    })
}

pub async fn resolve_from_asset_name(
    name: &str,
    req: &models::CreateUnrealProjectRequest,
    job_id: &Option<String>,
) -> Result<Option<PathBuf>, HttpResponse> {
    let downloads_base = find_downloads_directory();
    let mut asset_dir = find_asset_directory(&downloads_base, name);

    // Determine search directory based on UE version
    let mut search_dir = asset_dir.clone();
    if let Some(ref ue) = req.ue {
        let ue_trimmed = ue.trim();
        if !ue_trimmed.is_empty() {
            let candidate = asset_dir.join(ue_trimmed);
            if candidate.exists() {
                search_dir = candidate;
            }
        }
    }

    // Check if download is needed
    if needs_download(&asset_dir, &req.ue) {
        asset_dir = download_template_asset(name, &req.ue, job_id.as_deref()).await?;
        search_dir = determine_search_dir(&asset_dir, &req.ue);
    }

    println!("Searching for .uproject under: {}", search_dir.to_string_lossy());
    Ok(find_uproject_bfs(&search_dir, 8))
}

pub fn find_downloads_directory() -> PathBuf {
    let mut downloads_base = PathBuf::from("downloads");
    if !downloads_base.exists() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let alt = exe_dir.join("downloads");
                if alt.exists() {
                    downloads_base = alt;
                }
            }
        }
    }
    downloads_base
}

pub fn find_asset_directory(downloads_base: &Path, name: &str) -> PathBuf {
    let mut asset_dir = downloads_base.join(name);

    // Try case-insensitive match if exact name doesn't exist
    if !asset_dir.exists() && downloads_base.is_dir() {
        if let Ok(entries) = fs::read_dir(downloads_base) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                        if fname.eq_ignore_ascii_case(name) {
                            asset_dir = p;
                            break;
                        }
                    }
                }
            }
        }
    }
    asset_dir
}

pub fn needs_download(asset_dir: &Path, ue_version: &Option<String>) -> bool {
    if !asset_dir.exists() || !utils::is_download_complete(asset_dir) {
        return true;
    }

    if let Some(ue) = ue_version {
        let ue_trimmed = ue.trim();
        if !ue_trimmed.is_empty() {
            let version_dir = asset_dir.join(ue_trimmed);
            if !version_dir.exists() || !utils::is_download_complete(&version_dir) {
                return true;
            }
        }
    }

    false
}

pub async fn download_template_asset(
    name: &str,
    ue_version: &Option<String>,
    job_id: Option<&str>,
) -> Result<PathBuf, HttpResponse> {
    utils::emit_event(
        job_id,
        models::Phase::CreateDownloading,
        format!("Downloading '{}'", name),
        Some(0.0),
        None,
    );

    match utils::ensure_asset_downloaded_by_name(name, job_id, models::Phase::CreateDownloading).await {
        Ok(p) => {
            utils::emit_event(
                job_id,
                models::Phase::CreateDownloading,
                format!("Downloaded '{}'", name),
                Some(100.0),
                None,
            );
            Ok(p)
        }
        Err(err) => {
            eprintln!("{}", err);
            utils::emit_event(
                job_id,
                models::Phase::CreateError,
                format!("Failed to download '{}'", name),
                None,
                None,
            );
            Err(HttpResponse::NotFound().body(format!("{}", err)))
        }
    }
}

pub fn determine_search_dir(asset_dir: &Path, ue_version: &Option<String>) -> PathBuf {
    if let Some(ue) = ue_version {
        let ue_trimmed = ue.trim();
        if !ue_trimmed.is_empty() {
            let version_dir = asset_dir.join(ue_trimmed);
            if version_dir.exists() {
                return version_dir;
            }
        }
    }
    asset_dir.to_path_buf()
}

pub fn trim_quotes_and_expand_home(s: &str) -> String {
    let mut t = s.trim().to_string();

    // Remove surrounding quotes
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        t = t[1..t.len() - 1].to_string();
    }

    // Expand home directory
    if let Ok(home) = std::env::var("HOME") {
        if t.starts_with("~/") {
            t = t.replacen("~", &home, 1);
        }
        if t.contains("$HOME") {
            t = t.replace("$HOME", &home);
        }
    }
    t
}

pub fn find_uproject_bfs(start: &Path, max_depth: usize) -> Option<PathBuf> {
    use std::collections::VecDeque;

    if max_depth == 0 {
        return None;
    }

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((start.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        // If it's a file, check if it's a .uproject
        if dir.is_file() {
            if dir.extension().and_then(|s| s.to_str()) == Some("uproject") {
                return Some(dir);
            }
            continue;
        }

        if !dir.is_dir() {
            continue;
        }

        // Check current directory for .uproject files
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("uproject") {
                    return Some(p);
                }
            }
        }

        if depth >= max_depth {
            continue;
        }

        // Enqueue subdirectories (excluding common non-project dirs)
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                        let lname = name.to_ascii_lowercase();
                        if lname == "content" || lname == ".git" || lname == ".svn" {
                            continue;
                        }
                    }
                    queue.push_back((p, depth + 1));
                }
            }
        }
    }

    None
}

pub fn setup_output_directory(req: &models::CreateUnrealProjectRequest) -> Result<(PathBuf, PathBuf), HttpResponse> {
    let out_dir = PathBuf::from(trim_quotes_and_expand_home(&req.output_dir));

    if !out_dir.exists() {
        if let Err(e) = fs::create_dir_all(&out_dir) {
            return Err(HttpResponse::InternalServerError().body(
                format!("Failed to create output_dir: {}", e)
            ));
        }
    }

    let out_dir = std::fs::canonicalize(&out_dir).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|cwd| cwd.join(&out_dir))
            .unwrap_or(out_dir)
    });

    let new_project_dir = out_dir.join(&req.project_name);
    if let Err(e) = fs::create_dir_all(&new_project_dir) {
        return Err(HttpResponse::InternalServerError().body(
            format!("Failed to create new project directory: {}", e)
        ));
    }

    Ok((out_dir, new_project_dir))
}

pub fn handle_dry_run(
    req: &models::CreateUnrealProjectRequest,
    template_dir: &Path,
    new_project_dir: &Path,
    editor_path: &Path,
    target_uproject: &Path,
) -> HttpResponse {
    let exclude_names = ["Binaries", "DerivedDataCache", "Intermediate", "Saved", ".git", ".svn", ".vs"];
    let project_type = req.project_type.as_deref().unwrap_or("bp");

    let mut actions = vec![
        format!(
            "Copy '{}' -> '{}' (excluding {:?})",
            template_dir.to_string_lossy(),
            new_project_dir.to_string_lossy(),
            exclude_names
        ),
        format!(
            "Open with: {} {}{}",
            editor_path.to_string_lossy(),
            target_uproject.to_string_lossy(),
            if project_type == "bp" { " -NoCompile" } else { "" }
        ),
    ];

    let resp = models::CreateUnrealProjectResponse {
        ok: true,
        message: format!(
            "Dry run: would copy project files, then open project{}",
            if req.open_after_create.unwrap_or(false) {
                " (open_after_create=true)"
            } else {
                ""
            }
        ),
        command: actions.join(" | "),
        project_path: Some(new_project_dir.to_string_lossy().to_string()),
    };

    HttpResponse::Ok().json(resp)
}

pub fn copy_project_files(
    template_dir: &Path,
    new_project_dir: &Path,
    project_name: &str,
    template_path: &Path,
    job_id: &Option<String>,
) -> Result<(usize, usize), HttpResponse> {
    let exclude_names = ["Binaries", "DerivedDataCache", "Intermediate", "Saved", ".git", ".svn", ".vs"];

    // Count total files to copy
    let total_files = count_files_to_copy(template_dir, &exclude_names);

    println!(
        "[copy-start] {} -> {} ({} files, excluding {:?})",
        template_dir.to_string_lossy(),
        new_project_dir.to_string_lossy(),
        total_files,
        exclude_names
    );

    utils::emit_event(
        job_id.as_deref(),
        models::Phase::CreateCopying,
        format!("Creating new project at {}", new_project_dir.to_string_lossy()),
        Some(0.0),
        None,
    );

    let (copied, skipped) = perform_copy(
        template_dir,
        new_project_dir,
        project_name,
        template_path,
        &exclude_names,
        total_files,
        job_id,
    )?;

    println!(
        "[copy-finish] Copied {} files ({} skipped) to {}",
        copied,
        skipped,
        new_project_dir.to_string_lossy()
    );

    Ok((copied, skipped))
}

fn count_files_to_copy(template_dir: &Path, exclude_names: &[&str]) -> usize {
    let mut count = 0;
    for entry in walkdir::WalkDir::new(template_dir).into_iter().filter_map(|e| e.ok()) {
        let src_path = entry.path();
        let Ok(rel) = src_path.strip_prefix(template_dir) else { continue };

        if rel.as_os_str().is_empty() || should_exclude(rel, exclude_names) {
            continue;
        }

        if entry.file_type().is_file() {
            count += 1;
        }
    }
    count
}

fn should_exclude(rel_path: &Path, exclude_names: &[&str]) -> bool {
    use std::path::Component;

    if let Some(Component::Normal(os)) = rel_path.components().next() {
        let name = os.to_string_lossy().to_string();
        return exclude_names.iter().any(|ex| name.eq_ignore_ascii_case(ex));
    }
    false
}

fn perform_copy(
    template_dir: &Path,
    new_project_dir: &Path,
    project_name: &str,
    template_path: &Path,
    exclude_names: &[&str],
    total_files: usize,
    job_id: &Option<String>,
) -> Result<(usize, usize), HttpResponse> {
    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut last_logged_percent = 0u32;
    let mut last_log_instant = Instant::now();

    for entry in walkdir::WalkDir::new(template_dir).into_iter().filter_map(|e| e.ok()) {
        let src_path = entry.path();
        let Ok(rel) = src_path.strip_prefix(template_dir) else { continue };

        if rel.as_os_str().is_empty() {
            continue;
        }

        if should_exclude(rel, exclude_names) {
            skipped += 1;
            continue;
        }

        let dst_path = new_project_dir.join(rel);

        if entry.file_type().is_dir() {
            if let Err(e) = fs::create_dir_all(&dst_path) {
                return Err(HttpResponse::InternalServerError().body(
                    format!("Failed to create dir {}: {}", dst_path.to_string_lossy(), e)
                ));
            }
        } else if entry.file_type().is_file() {
            let final_dst = if src_path.extension().and_then(|s| s.to_str()) == Some("uproject") {
                new_project_dir.join(format!("{}.uproject", project_name))
            } else {
                dst_path
            };

            if let Some(parent) = final_dst.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Err(HttpResponse::InternalServerError().body(
                        format!("Failed to create parent dir {}: {}", parent.to_string_lossy(), e)
                    ));
                }
            }

            if let Err(e) = fs::copy(src_path, &final_dst) {
                return Err(HttpResponse::InternalServerError().body(
                    format!("Failed to copy {} -> {}: {}", src_path.to_string_lossy(), final_dst.to_string_lossy(), e)
                ));
            }

            copied += 1;

            // Log progress
            if total_files > 0 {
                let percent = ((copied as f64 / total_files as f64) * 100.0).floor() as u32;
                if percent >= last_logged_percent + 5 || last_log_instant.elapsed().as_secs() >= 2 {
                    println!("[copy-progress] {}/{} ({}%) - {}", copied, total_files, percent, rel.to_string_lossy());
                    last_logged_percent = percent;
                    last_log_instant = Instant::now();
                    utils::emit_event(
                        job_id.as_deref(),
                        models::Phase::CreateCopying,
                        format!("{} / {}", copied, total_files),
                        Some(percent as f32),
                        None,
                    );
                }
            }
        } else if entry.file_type().is_symlink() {
            skipped += 1;
        }
    }

    Ok((copied, skipped))
}

pub fn finalize_uproject(
    new_project_dir: &Path,
    req: &models::CreateUnrealProjectRequest,
    template_path: &Path,
) -> PathBuf {
    let new_uproject = new_project_dir.join(format!("{}.uproject", req.project_name));

    // Fallback if rename didn't occur
    let target_uproject = if new_uproject.exists() {
        new_uproject.clone()
    } else {
        let fallback = new_project_dir.join(template_path.file_name().unwrap_or_default());
        if !fallback.exists() {
            let _ = fs::copy(template_path, &fallback);
        }
        fallback
    };

    // Update project metadata
    update_project_metadata(&target_uproject, req);

    target_uproject
}

fn update_project_metadata(uproject_path: &Path, req: &models::CreateUnrealProjectRequest) {
    let Ok(json_text) = fs::read_to_string(uproject_path) else { return };

    // Update display/friendly name
    if json_text.contains("\"FileVersion\"") || json_text.contains("\"EngineAssociation\"") {
        let updated = json_text
            .replace("\"DisplayName\":\"", &format!("\"DisplayName\":\"{}",req.project_name))
            .replace("\"FriendlyName\":\"", &format!("\"FriendlyName\":\"{}",  req.project_name));

        if updated != json_text {
            let _ = fs::write(uproject_path, &updated);
        }
    }

    // Set EngineAssociation if UE version specified
    if let Some(ue) = &req.ue {
        set_engine_association(uproject_path, ue);
    }
}

fn set_engine_association(uproject_path: &Path, ue_version: &str) {
    let mut ue = ue_version.trim().to_string();
    if ue.starts_with("UE_") {
        ue = ue[3..].to_string();
    }

    let parts: Vec<&str> = ue.split('.').collect();
    if parts.len() < 2 {
        return;
    }

    let major_minor = format!("{}.{}", parts[0].trim(), parts[1].trim());

    let Ok(text) = fs::read_to_string(uproject_path) else { return };
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else { return };

    if let Some(obj) = json.as_object_mut() {
        obj.insert("EngineAssociation".to_string(), serde_json::Value::String(major_minor));
        if let Ok(pretty) = serde_json::to_string_pretty(&json) {
            let _ = fs::write(uproject_path, pretty);
        }
    }
}

pub fn build_editor_command(
    editor_path: &Path,
    uproject_path: &Path,
    project_type: &Option<String>,
) -> String {
    let ptype = project_type.as_deref().unwrap_or("bp");
    format!(
        "{} {}{}",
        editor_path.to_string_lossy(),
        uproject_path.to_string_lossy(),
        if ptype == "bp" { " -NoCompile" } else { "" }
    )
}

pub fn execute_project_open(
    req: &models::CreateUnrealProjectRequest,
    copied: usize,
    skipped: usize,
    command: String,
    project_dir: &Path,
) -> HttpResponse {
    let project_type = req.project_type.as_deref().unwrap_or("bp");
    let open_after = req.open_after_create.unwrap_or(false);

    if !open_after {
        let resp = models::CreateUnrealProjectResponse {
            ok: true,
            message: format!(
                "Project created ({} files, {} skipped). Not opening (open_after_create=false).",
                copied, skipped
            ),
            command,
            project_path: Some(project_dir.to_string_lossy().to_string()),
        };
        return HttpResponse::Ok().json(resp);
    }

    // Parse command and spawn process
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return HttpResponse::InternalServerError().body("Invalid command");
    }

    let mut cmd = std::process::Command::new(parts[0]);
    for arg in &parts[1..] {
        cmd.arg(arg);
    }

    match cmd.spawn() {
        Ok(_) => {
            let resp = models::CreateUnrealProjectResponse {
                ok: true,
                message: format!(
                    "Project created ({} files, {} skipped). Unreal Editor is launching...",
                    copied, skipped
                ),
                command,
                project_path: Some(project_dir.to_string_lossy().to_string()),
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => {
            let resp = models::CreateUnrealProjectResponse {
                ok: true,
                message: format!(
                    "Project created ({} files, {} skipped). Failed to launch UnrealEditor: {}",
                    copied, skipped, e
                ),
                command,
                project_path: Some(project_dir.to_string_lossy().to_string()),
            };
            HttpResponse::Ok().json(resp)
        }
    }
}

pub async fn download_asset_handler(path: web::Path<(String, String, String)>, query: Query<HashMap<String, String>>) -> Result<HttpResponse, HttpResponse> {
    let (namespace, asset_id, artifact_id) = path.into_inner();
    let job_id = query.get("jobId").cloned().or_else(|| query.get("job_id").cloned());
    let ue_major_minor_version = query.get("ue").cloned();

    // If already cancelled before we start, exit early
    if check_if_job_is_cancelled(job_id.as_deref()) {
        emit_event(job_id.as_deref(), models::Phase::Cancelled, "Job cancelled", None, None);
        if let Some(ref job) = job_id { acknowledge_cancel(job); }
        return Err(HttpResponse::Ok().body("cancelled"));
    }


    // Authenticate with Epic services
    let mut epic_services = create_epic_games_services();
    if !try_cached_login(&mut epic_services).await {
        epic_authenticate(&mut epic_services).await;
    }

    // Emit start event with a user-friendly asset title if available.
    let asset_name = utils::get_friendly_asset_name(&namespace, &asset_id, &artifact_id, &mut epic_services).await;
    emit_event(
        job_id.as_deref(),
        models::Phase::DownloadStart,
        format!("Starting to download asset: {}", asset_name),
        Some(0.0),
        None);

    // Fetch manifest for the specified asset/artifact
    let manifest_res = epic_services.fab_asset_manifest(&artifact_id, &namespace, &asset_id, None).await;
    let manifests = match manifest_res {
        Ok(m) => m,
        Err(e) => {
            emit_event(job_id.as_deref(), models::Phase::DownloadError, format!("Failed to fetch manifest: {:?}", e), None, None);
            return Err(HttpResponse::BadRequest().body(format!("Failed to fetch manifest: {:?}", e)));
        }
    };

    for manifest in manifests.iter() {
        // Get a download URL
        for url in manifest.distribution_point_base_urls.iter() {
            // Check if job has been requested to cancel
            if check_if_job_is_cancelled(job_id.as_deref()) {
                // If requested to cancel, cancel job
                emit_event(job_id.as_deref(), models::Phase::Cancelled, "Job cancelled", None, None);
                if let Some(ref j) = job_id { acknowledge_cancel(j); }
                return Err(HttpResponse::Ok().body("cancelled"));
            }

            if let Ok(mut download_manifest) = epic_services.fab_download_manifest(manifest.clone(), url).await {
                // Ensure SourceURL present for downloader (some tooling relies on it)
                use std::collections::HashMap;
                if let Some(ref mut fields) = download_manifest.custom_fields {
                    fields.insert("SourceURL".to_string(), url.clone());
                } else {
                    let mut map = HashMap::new();
                    map.insert("SourceURL".to_string(), url.clone());
                    download_manifest.custom_fields = Some(map);
                }

                let friendly_folder_name = get_friendly_folder_name(asset_name.clone());
                let folder_name = friendly_folder_name.clone().unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id));

                let mut download_directory_full_path = get_default_downloads_dir_path().join(folder_name);
                if let Some(ref major_minor_version) = ue_major_minor_version {
                    if major_minor_version.trim().is_empty() == false {
                        // Create folder called specific version of asset
                        download_directory_full_path = download_directory_full_path.join(major_minor_version.trim());
                    }
                }

                // Progress callback: forward file completion percentage over WS
                let progress_callback: Option<ProgressFn> = job_id.as_deref().map(|jid| {
                    let jid = jid.to_string();
                    let f: ProgressFn = std::sync::Arc::new(move |percentage_complete: u32, msg: String| {
                        emit_event(Some(&jid), models::Phase::DownloadProgress, format!("{}", msg), Some(percentage_complete as f32), None);
                    });
                    f
                });

                match download_asset(&download_manifest, url.as_str(), &download_directory_full_path, progress_callback, job_id.as_deref()).await {
                    Ok(_) => {
                        println!("Download complete");

                        if utils::check_if_job_is_cancelled(job_id.as_deref()) {
                            // Remove the incomplete asset folder so partial files are not left behind
                            if let Err(err) = fs::remove_dir_all(&download_directory_full_path) {
                                eprintln!("Cleanup warning: failed to remove incomplete asset folder {}: {:?}", download_directory_full_path.display(), err);
                            }
                            utils::emit_event(job_id.as_deref(), models::Phase::Cancelled, "Job cancelled", None, None);
                            if let Some(ref j) = job_id { utils::acknowledge_cancel(j); }
                            return Err(HttpResponse::Ok().body("cancelled"));
                        }

                        // After a successful download, update the cached FAB list (if present)
                        // to mark this asset and specific version as downloaded, so the UI can
                        // reflect the state without requiring a full refresh.
                        let fab_cache_file_path = get_fab_cache_file_path();
                        update_fab_cache_json(namespace, asset_id, artifact_id, ue_major_minor_version, friendly_folder_name, &fab_cache_file_path);

                        emit_event(job_id.as_deref(), models::Phase::DownloadComplete, "Download complete", Some(100.0), None);
                        // TODO: Should we really acknowledge cancel if the download has completed?
                        if let Some(ref j) = job_id { utils::acknowledge_cancel(j); }
                        // TODO: The below was retuning an Err instead of Ok, should it be an Err?
                        return Ok(HttpResponse::Ok().body("Download complete"))
                    },
                    Err(e) => {
                        if utils::check_if_job_is_cancelled(job_id.as_deref()) {
                            // Remove the incomplete asset folder so partial files are not left behind
                            if let Err(err) = fs::remove_dir_all(&download_directory_full_path) {
                                eprintln!("Cleanup warning: failed to remove incomplete asset folder {}: {:?}", download_directory_full_path.display(), err);
                            }
                            utils::emit_event(job_id.as_deref(), models::Phase::Cancelled, "Job cancelled", None, None);
                            if let Some(ref j) = job_id { utils::acknowledge_cancel(j); }
                            return Err(HttpResponse::Ok().body("cancelled"));
                        }
                        eprintln!("Download failed from {}: {:?}", url, e);
                        continue;
                    }
                }
            }
        }
    }

    utils::emit_event(job_id.as_deref(), models::Phase::DownloadError, "Unable to download asset from any distribution point", None, None);
    Ok(HttpResponse::InternalServerError().body("Unable to download asset from any distribution point"))
}
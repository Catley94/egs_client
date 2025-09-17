//! Utilities: authentication, token caching, Fab library access, and file downloads.
//!
//! This module centralizes the heavy lifting used by the HTTP API layer:
//! - Auth-code login flow and cached token reuse (try_cached_login)
//! - Serialization of UserData tokens to a local file with safe permissions
//! - Convenience wrappers for EGS endpoints (account details/info, library items)
//! - Robust downloader assembling files from chunk parts described by a DownloadManifest
//!
//! Key concepts and files:
//! - Token cache: ~/.egs_client_tokens.json (0600 on Unix)
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

use std::io;
use egs_api::api::types::account::{AccountData, AccountInfo, UserData};
use egs_api::api::types::fab_library::FabLibrary;
use egs_api::EpicGames;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use egs_api::api::types::download_manifest::DownloadManifest;

const EPIC_LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

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
/// - Uses $HOME/.egs_client_tokens.json when HOME is set.
/// - Falls back to ./.egs_client_tokens.json otherwise.
///
/// Future improvements (TODO):
/// - Move to a proper cache/config directory and provide a "clear credentials" helper.
fn token_cache_path() -> PathBuf {
    // Store tokens in the user's home directory
    // TODO: Change this to a location properly in cache, or local to the project
    // TODO: Also add a way to clear the cached credentials
    if let Ok(home) = std::env::var("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".egs_client_tokens.json");
        p
    } else {
        // Fallback: current directory
        PathBuf::from(".egs_client_tokens.json")
    }
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

pub async fn download_asset(dm: &DownloadManifest, _base_url: &str, out_root: &Path, progress: Option<ProgressFn>) -> Result<(), anyhow::Error> {
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

    let client = reqwest::Client::new();

    let files: Vec<_> = dm.files().into_iter().collect();
    let total_files = files.len();
    if total_files == 0 {
        return Err(anyhow::anyhow!("download manifest contains no files"));
    }

    // Setup file-level concurrency
    let file_sema = Arc::new(Semaphore::new(max_files));
    let mut join = JoinSet::new();

    #[derive(Default)]
    struct Totals { downloaded: usize, skipped_zero: usize, up_to_date: usize }
    let totals = Arc::new(tokio::sync::Mutex::new(Totals::default()));

        // Track completed files across concurrent tasks to compute overall percent
        let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for (file_idx, (filename, file)) in files.into_iter().enumerate() {
        let permit_owner = file_sema.clone().acquire_owned().await.expect("semaphore closed");
        let client = client.clone();
        let temp_dir = temp_dir.clone();
        let out_root = out_root.to_path_buf();
        let totals = totals.clone();
        let completed = completed.clone();
        let progress = progress.clone();
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
                let guid = part.guid.clone();
                let link = part.link.clone();
                let client = client.clone();
                let temp_dir = temp_dir.clone();
                let chunk_permit_owner = chunk_sema.clone().acquire_owned().await.expect("chunk sema closed");
                chunk_join.spawn(async move {
                    let _p = chunk_permit_owner; // hold permit until end
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
                    let mut resp = client.get(url.clone()).send().await;
                    if resp.is_err() { resp = client.get(url.clone()).send().await; }
                    let resp = resp.map_err(|e| anyhow::anyhow!("chunk request failed for {}: {}", guid, e))?;
                    let resp = resp.error_for_status().map_err(|e| anyhow::anyhow!("chunk HTTP {} for {}", e.status().unwrap_or_default(), guid))?;
                    let bytes = resp.bytes().await.map_err(|e| anyhow::anyhow!("read chunk {}: {}", guid, e))?;
                    if let Some(parent) = chunk_path.parent() { let _ = std::fs::create_dir_all(parent); }
                    std::fs::write(&chunk_path, &bytes)?;
                    Ok(())
                });
            }

            // Wait all chunks
            while let Some(res) = chunk_join.join_next().await { res??; }
            println!("\r  chunks: {}/{} (100%) - done                    ", total_chunks, total_chunks);

            // Assemble
            let mut out = std::fs::File::create(&tmp_out_path)?;
            let mut hasher = Sha1::new();
            let total_bytes: u128 = file.file_chunk_parts.iter().map(|p| p.size as u128).sum();
            let mut written: u64 = 0;
            for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
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
    while let Some(res) = join.join_next().await { res??; }

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

    Ok(())
}

/// Sanitize a title for use as a folder name (mirrors logic in download_asset and refresh).
fn sanitize_title_for_folder(s: &str) -> String {
    let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let replaced = s.replace(&illegal[..], "_");
    let trimmed = replaced.trim().trim_matches('.').to_string();
    trimmed
}
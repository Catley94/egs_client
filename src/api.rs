//! HTTP API layer for Epic Games Store (EGS) Fab assets.
//!
//! This module exposes a small set of Actix Web endpoints to:
//! - Retrieve your Fab (Marketplace) library from Epic Games Services (EGS),
//! - Cache the library as JSON for faster subsequent loads,
//! - Download a specific asset by namespace/asset/artifact identifiers.
//!
//! Quick navigation:
//! - GET /get-fab-list — returns the cached list if available, otherwise refreshes first.
//! - GET /refresh-fab-list — forces a refresh from EGS and updates the cache file.
//! - GET /download-asset/{namespace}/{asset_id}/{artifact_id} — downloads an asset to the downloads/ directory.
//!
//! Requirements and environment:
//! - Authentication to EGS is handled via the utils module. The service attempts to reuse cached tokens and may fall back to an auth-code flow.
//! - A cache file is kept at cache/fab_list.json.
//! - Downloads are written to downloads/<Asset Title>/.
//!
//! ASCII diagram of the typical flow for listing and downloading:
//!
//!  Client           API (this module)               EGS (remote)
//!    |   GET /get-fab-list                             |
//!    |-----------------------------------------------> |
//!    |   cache hit? yes -> return JSON                 |
//!    |   cache hit? no  -> /refresh-fab-list           |
//!    |                           |                     |
//!    |                           |  authenticate       |
//!    |                           |-------------------> |
//!    |                           |  fetch library      |
//!    |                           |<------------------- |
//!    |   write cache, return JSON                      |
//!    |<----------------------------------------------- |
//!    |                                                 |
//!    |   GET /download-asset/{ids}                     |
//!    |-----------------------------------------------> |
//!    |   ensure auth, fetch manifests                  |
//!    |                           |-------------------> |
//!    |                           |<------------------- |
//!    |   pick distribution URL, download files         |
//!    |   write to downloads/                           |
//!    |<----------------------------------------------- |
//!
//! Helpful links:
//! - Actix Web: https://actix.rs/
//! - Epic API crate used here (egs-api): https://crates.io/crates/egs-api
//! - Project README (if available) for global setup.
//!
//! Notes:
//! - This is a thin orchestration layer; most heavy lifting (auth, library lookups, downloads)
//!   is implemented in crate::utils and the egs_api crate.
//! - All endpoints return HttpResponse and are designed for a UI frontend to consume.

use actix_web::{get, HttpResponse, web};
use colored::Colorize;
use crate::utils;

use std::fs;
use std::io::Read;
use serde_json;

/// Directory where the Fab library cache is stored.
const FAB_CACHE_DIR: &str = "cache";
/// File containing the cached Fab library JSON.
const FAB_CACHE_FILE: &str = "cache/fab_list.json";

/// Returns the user's Fab library, preferring a cached JSON file when possible.
///
/// Behavior:
/// - If cache/fab_list.json exists and is readable, the raw JSON bytes are sent back as application/json.
/// - Otherwise, it falls back to performing a refresh (same behavior as /refresh-fab-list).
///
/// Example (curl):
/// - curl -s http://localhost:8080/get-fab-list | jq
///
/// Status codes:
/// - 200 OK on success (JSON body)
/// - 200 OK with a plain string body in some edge cases (e.g., "No details found")
#[get("/get-fab-list")]
pub async fn get_fab_list() -> HttpResponse {
    let path = std::path::Path::new(FAB_CACHE_FILE);
    if path.exists() {
        if let Ok(mut f) = fs::File::open(path) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                println!("Using cached FAB list from {}", FAB_CACHE_FILE);
                // Return raw JSON contents
                return HttpResponse::Ok()
                    .content_type("application/json")
                    .body(buf);
            }
        }
    }
    // Fallback: refresh and cache
    handle_refresh_fab_list().await
}

/// Forces a refresh of the user's Fab library from Epic Games Services and caches it.
///
/// This endpoint performs authentication (attempts cached token first), retrieves account
/// details and Fab library items, serializes them to cache/fab_list.json, and returns the
/// JSON list in the response.
///
/// Example (curl):
/// - curl -s http://localhost:8080/refresh-fab-list | jq '.results | length'
#[get("/refresh-fab-list")]
pub async fn refresh_fab_list() -> HttpResponse {
    // Respond with the list of Fab Assets and cache it

    handle_refresh_fab_list().await
}

/// Internal helper that refreshes the Fab library without initiating any downloads.
///
/// Returns a summary list (JSON) suitable for UI consumption. On auth failure or missing
/// details, returns a 200 OK with a short message body describing the condition.
pub async fn handle_refresh_fab_list() -> HttpResponse {
    // Try to use cached refresh token first (no browser, no copy-paste)
    let mut epic_games_services = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic_games_services).await {
        // If cached token is not available or invalid, we obtain an auth code from utils,
        // perform the code exchange, and then finalize login. On success, tokens are saved
        // for subsequent runs to avoid repeating the interactive step.
        let auth_code = utils::get_auth_code();

        // Authenticate with Epic's Servers using the code
        if epic_games_services.auth_code(None, Some(auth_code)).await {
            println!("Logged in with provided auth code");
        }
        // Complete login; the SDK should populate user_details with tokens
        let _ = epic_games_services.login().await;

        // Persist tokens for next runs
        let ud = epic_games_services.user_details();
        if let Err(e) = utils::save_user_details(&ud) {
            eprintln!("Warning: failed to save tokens: {}", e);
        }
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
                    // Save to cache file for faster subsequent loads and offline-friendly UI.
                    if let Ok(json_bytes) = serde_json::to_vec_pretty(&retrieved_assets) {
                        if let Some(parent) = std::path::Path::new(FAB_CACHE_DIR).parent() { let _ = fs::create_dir_all(parent); }
                        let _ = fs::create_dir_all(FAB_CACHE_DIR);
                        if let Err(e) = fs::write(FAB_CACHE_FILE, &json_bytes) {
                            eprintln!("Warning: failed to write FAB cache: {}", e);
                        }
                    } else {
                        eprintln!("Warning: failed to serialize FAB library for cache");
                    }
                    // Return the library items so the UI can populate list/images
                    return HttpResponse::Ok().json(&retrieved_assets);

                    // Reached only if json() above wasn't returned; keep OK fallback
                    HttpResponse::Ok().finish()
                }
            }
        }
    }
}

/// Downloads a specific Fab asset to the local filesystem.
///
/// Route:
/// - GET /download-asset/{namespace}/{asset_id}/{artifact_id}
///
/// Parameters:
/// - namespace: String — asset namespace in Fab
/// - asset_id: String — the Fab asset identifier
/// - artifact_id: String — concrete artifact/version identifier
///
/// Behavior:
/// - Ensures valid authentication (reuses cached tokens when possible).
/// - Fetches the asset's manifests and iterates over available distribution points.
/// - For each distribution point, requests the download manifest and injects a
///   custom field SourceURL used by the downstream downloader.
/// - Attempts to resolve a human-friendly output directory using the asset title,
///   sanitized for filesystem safety; falls back to a namespace-asset-artifact folder name.
/// - Invokes utils::download_asset to perform the actual download into downloads/.
///
/// Returns:
/// - 200 OK "Download complete" on success.
/// - 400 Bad Request if the manifest cannot be fetched.
/// - 500 InternalServerError if all distribution points fail.
///
/// Example (curl):
/// - curl -v http://localhost:8080/download-asset/89efe5924d3d467c839449ab6ab52e7f/28b7df0e7f5e4202be89a20d362860c3/Industryf4a3f3ff297fV1
#[get("/download-asset/{namespace}/{asset_id}/{artifact_id}")]
pub async fn download_asset(path: web::Path<(String, String, String)>) -> HttpResponse {
    let (namespace, asset_id, artifact_id) = path.into_inner();

    let mut epic = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic).await {
        let auth_code = utils::get_auth_code();
        let _ = epic.auth_code(None, Some(auth_code)).await;
        let _ = epic.login().await;
        let _ = utils::save_user_details(&epic.user_details());
    }

    // Fetch manifest for the specified asset/artifact
    let manifest_res = epic.fab_asset_manifest(&artifact_id, &namespace, &asset_id, None).await;
    let manifests = match manifest_res {
        Ok(m) => m,
        Err(e) => {
            return HttpResponse::BadRequest().body(format!("Failed to fetch manifest: {:?}", e));
        }
    };

    for man in manifests.iter() {
        for url in man.distribution_point_base_urls.iter() {
            if let Ok(mut dm) = epic.fab_download_manifest(man.clone(), url).await {
                // Ensure SourceURL present for downloader (some tooling relies on it)
                use std::collections::HashMap;
                if let Some(ref mut fields) = dm.custom_fields {
                    fields.insert("SourceURL".to_string(), url.clone());
                } else {
                    let mut map = HashMap::new();
                    map.insert("SourceURL".to_string(), url.clone());
                    dm.custom_fields = Some(map);
                }

                // Resolve a human-friendly title for folder name, if available.
                let mut title_folder: Option<String> = None;
                // Try to use the library list to find the matching asset by IDs
                if let Some(details) = utils::get_account_details(&mut epic).await {
                    if let Some(lib) = utils::get_fab_library_items(&mut epic, details).await {
                        if let Some(asset) = lib.results.iter().find(|a| a.asset_namespace == namespace && a.asset_id == asset_id) {
                            // Verify the artifact belongs to this asset's versions
                            if asset.project_versions.iter().any(|v| v.artifact_id == artifact_id) {
                                let mut t = asset.title.clone();
                                // Replace characters illegal on common filesystems.
                                let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
                                t = t.replace(&illegal[..], "_");
                                // Also trim leading/trailing spaces and dots (Windows quirk).
                                let t = t.trim().trim_matches('.').to_string();
                                if !t.is_empty() {
                                    title_folder = Some(t);
                                }
                            }
                        }
                    }
                }

                let folder_name = title_folder.unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id));
                let out_root = std::path::Path::new("downloads").join(folder_name);
                match utils::download_asset(&dm, url.as_str(), &out_root).await {
                    Ok(_) => return HttpResponse::Ok().body("Download complete"),
                    Err(e) => {
                        eprintln!("Download failed from {}: {:?}", url, e);
                        continue;
                    }
                }
            }
        }
    }

    HttpResponse::InternalServerError().body("Unable to download asset from any distribution point")
}


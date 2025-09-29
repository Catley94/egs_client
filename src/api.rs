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

use actix_web::{get, post, HttpResponse, web, Responder, HttpRequest};
use crate::utils;
use crate::models;
use crate::utils::EPIC_LOGIN_URL;

use std::fs;
use std::io::Read;
use serde::{Deserialize};
use serde_json;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::collections::{HashMap, VecDeque};
use actix_web::web::Query;
use actix_web_actors::ws;
use egs_api::EpicGames;
use crate::utils::get_sender;

/// Default directory names used when no config/environment override is provided.
pub const DEFAULT_CACHE_DIR_NAME: &str = "cache";
pub const DEFAULT_DOWNLOADS_DIR_NAME: &str = "downloads";

/// Note: cache and downloads directories are configurable; see helpers below for effective paths.



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
/// Returns the user's Fab library, preferring a cached JSON file when possible.
///
/// Behavior:
/// - If cache/fab_list.json exists and is readable, the raw JSON (enriched with local flags when possible)
///   is returned as application/json.
/// - Otherwise, it falls back to performing a refresh (same behavior as /refresh-fab-list).
///
/// Example (curl):
/// - curl -s http://localhost:8080/get-fab-list | jq
///
/// Status codes:
/// - 200 OK on success (JSON body)
#[get("/get-fab-list")]
pub async fn get_fab_list() -> HttpResponse {
    let path = utils::fab_cache_file();
    if path.exists() {
        if let Ok(mut f) = fs::File::open(&path) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                // Try to parse and re-annotate downloaded flags based on current filesystem state.
                match serde_json::from_slice::<serde_json::Value>(&buf) {
                    Ok(mut val) => {
                        let (_total, _marked, changed) = utils::annotate_downloaded_flags(&mut val);
                        if changed {
                            if let Ok(bytes) = serde_json::to_vec_pretty(&val) {
                                if let Err(e) = fs::write(&path, &bytes) {
                                    eprintln!("Warning: failed to update FAB cache while serving: {}", e);
                                }
                            }
                            println!("Using cached FAB list from {} (re-annotated)", path.display());
                        } else {
                            println!("Using cached FAB list from {} (no changes)", path.display());
                        }
                        return HttpResponse::Ok().json(val);
                    }
                    Err(_) => {
                        // If parsing failed, fall back to returning raw bytes.
                        println!("Using cached FAB list from {} (raw)", path.display());
                        return HttpResponse::Ok()
                            .content_type("application/json")
                            .body(buf);
                    }
                }
            }
        }
    }
    // Fallback: refresh and cache
    utils::handle_refresh_fab_list().await
}

/// WebSocket endpoint used to stream progress/events to the Flutter UI.
///
/// Query params:
/// - jobId or job_id: logical job identifier; messages are broadcast per job.
///
/// Behavior:
/// - Subscribes client to a per-job broadcast channel.
/// - Flushes buffered events for late subscribers, then streams live updates.
#[get("/ws")]
pub async fn ws_endpoint(req: HttpRequest, stream: web::Payload, query: web::Query<HashMap<String, String>>) -> Result<HttpResponse, actix_web::Error> {
    let job_id = query.get("jobId").cloned().or_else(|| query.get("job_id").cloned()).unwrap_or_else(|| "default".to_string());
    println!("[WS] connect: job_id={}, peer={}", job_id, req.peer_addr().map(|a| a.to_string()).unwrap_or_else(|| "unknown".into()));
    let rx = get_sender(&job_id).subscribe();
    let resp = ws::start(utils::WsSession { rx, job_id }, &req, stream);
    resp
}

/// Forces a refresh of the user's Fab library from Epic Games Services and caches it.
///
/// This endpoint performs authentication (attempts cached token first), retrieves account
/// details and Fab library items, serializes them to cache/fab_list.json, and returns the
/// JSON list in the response.
///
/// Example (curl):
/// - curl -s http://localhost:8080/refresh-fab-list | jq '.results | length'
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
    utils::handle_refresh_fab_list().await
}

#[get("/auth/start")]
pub async fn auth_start() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "auth_url": EPIC_LOGIN_URL,
        "message": "Open this URL in your browser and sign in to Epic Games; copy the authorizationCode from the JSON page."
    }))
}

#[derive(Deserialize)]
pub struct AuthCompleteRequest { pub code: String }

#[post("/auth/complete")]
pub async fn auth_complete(body: web::Json<AuthCompleteRequest>) -> HttpResponse {
    let code = body.code.trim().trim_matches('"').to_string();
    if code.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "ok": false,
            "message": "Missing 'code' field in body"
        }));
    }
    let mut epic = utils::create_epic_games_services();
    let auth_ok = epic.auth_code(None, Some(code)).await;
    if !auth_ok {
        return HttpResponse::Unauthorized().json(serde_json::json!({
            "ok": false,
            "message": "Auth code was not accepted by Epic servers"
        }));
    }
    // Complete login and persist tokens
    let logged_in = epic.login().await;
    if !logged_in {
        return HttpResponse::Unauthorized().json(serde_json::json!({
            "ok": false,
            "message": "Login failed after exchanging auth code"
        }));
    }
    let ud = epic.user_details();
    if let Err(e) = utils::save_user_details(&ud) {
        eprintln!("Warning: failed to save tokens: {}", e);
    }
    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "message": "Authentication successful"
    }))
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
pub async fn download_asset(path: web::Path<(String, String, String)>, query: web::Query<HashMap<String, String>>) -> HttpResponse {
    match download_asset_handler(path, query).await {
        Ok(value) => value,
        Err(value) => return value,
    }
}

async fn download_asset_handler(path: web::Path<(String, String, String)>, query: Query<HashMap<String, String>>) -> Result<HttpResponse, HttpResponse> {
    let (namespace, asset_id, artifact_id) = path.into_inner();
    let job_id = query.get("jobId").cloned().or_else(|| query.get("job_id").cloned());
    let ue_major_minor_version = query.get("ue").cloned();

    // If already cancelled before we start, exit early
    if utils::is_cancelled(job_id.as_deref()) {
        utils::emit_event(job_id.as_deref(), "cancelled", "Job cancelled", None, None);
        if let Some(ref j) = job_id { utils::clear_cancel(j); }
        return Err(HttpResponse::Ok().body("cancelled"));
    }


    let mut epic_services = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic_services).await {
        epic_authenticate(&mut epic_services).await;
    }

    // Emit start event with a user-friendly asset title if available.
    let asset_name = get_friendly_asset_name(&namespace, &asset_id, &artifact_id, &mut epic_services).await;
    utils::emit_event(
        job_id.as_deref(),
        "download:start",
        format!("Starting download {}", asset_name),
        Some(0.0),
        None);

    // Fetch manifest for the specified asset/artifact
    let manifest_res = epic_services.fab_asset_manifest(&artifact_id, &namespace, &asset_id, None).await;
    let manifests = match manifest_res {
        Ok(m) => m,
        Err(e) => {
            utils::emit_event(job_id.as_deref(), "download:error", format!("Failed to fetch manifest: {:?}", e), None, None);
            return Err(HttpResponse::BadRequest().body(format!("Failed to fetch manifest: {:?}", e)));
        }
    };

    for man in manifests.iter() {
        // Get a download URL
        for url in man.distribution_point_base_urls.iter() {
            // Check if job has been requested to cancel
            if utils::is_cancelled(job_id.as_deref()) {
                // If requested to cancel, cancel job
                utils::emit_event(job_id.as_deref(), "cancelled", "Job cancelled", None, None);
                if let Some(ref j) = job_id { utils::clear_cancel(j); }
                return Err(HttpResponse::Ok().body("cancelled"));
            }

            if let Ok(mut dm) = epic_services.fab_download_manifest(man.clone(), url).await {
                // Ensure SourceURL present for downloader (some tooling relies on it)
                use std::collections::HashMap;
                if let Some(ref mut fields) = dm.custom_fields {
                    fields.insert("SourceURL".to_string(), url.clone());
                } else {
                    let mut map = HashMap::new();
                    map.insert("SourceURL".to_string(), url.clone());
                    dm.custom_fields = Some(map);
                }

                let title_folder = get_friendly_folder_name(asset_name.clone());


                // // Try to use the library list to find the matching asset by IDs
                // if let Some(details) = utils::get_account_details(&mut epic_services).await {
                //     if let Some(lib) = utils::get_fab_library_items(&mut epic_services, details).await {
                //         if let Some(asset) = lib.results.iter().find(|a| a.asset_namespace == namespace && a.asset_id == asset_id) {
                //             // Verify the artifact belongs to this asset's versions
                //             if asset.project_versions.iter().any(|v| v.artifact_id == artifact_id) {
                //                 let mut t = asset.title.clone();
                //                 // Replace characters illegal on common filesystems.
                //                 let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
                //                 t = t.replace(&illegal[..], "_");
                //                 // Also trim leading/trailing spaces and dots (Windows quirk).
                //                 let t = t.trim().trim_matches('.').to_string();
                //                 if !t.is_empty() {
                //                     title_folder = Some(t);
                //                 }
                //             }
                //         }
                //     }
                // }

                let folder_name = title_folder.clone().unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id));
                let mut download_directory_full_path = utils::default_downloads_dir().join(folder_name);
                if let Some(ref major_minor_version) = ue_major_minor_version {
                    if !major_minor_version.trim().is_empty() {
                        // Create folder called specific version of asset
                        download_directory_full_path = download_directory_full_path.join(major_minor_version.trim());
                    }
                }

                // Progress callback: forward file completion percentage over WS
                let progress_callback: Option<utils::ProgressFn> = job_id.as_deref().map(|jid| {
                    let jid = jid.to_string();
                    let f: utils::ProgressFn = std::sync::Arc::new(move |percentage_complete: u32, msg: String| {
                        utils::emit_event(Some(&jid), "download:progress", format!("{}", msg), Some(percentage_complete as f32), None);
                    });
                    f
                });

                match utils::download_asset(&dm, url.as_str(), &download_directory_full_path, progress_callback, job_id.as_deref()).await {
                    Ok(_) => {
                        println!("Download complete");

                        // After a successful download, update the cached FAB list (if present)
                        // to mark this asset and specific version as downloaded, so the UI can
                        // reflect the state without requiring a full refresh.
                        let cache_path = utils::fab_cache_file();
                        update_fab_cache_json(namespace, asset_id, artifact_id, ue_major_minor_version, title_folder, &cache_path);

                        utils::emit_event(job_id.as_deref(), "download:complete", "Download complete", Some(100.0), None);
                        if let Some(ref j) = job_id { utils::clear_cancel(j); }
                        return Err(HttpResponse::Ok().body("Download complete"))
                    },
                    Err(e) => {
                        if utils::is_cancelled(job_id.as_deref()) {
                            // Remove the incomplete asset folder so partial files are not left behind
                            if let Err(err) = fs::remove_dir_all(&download_directory_full_path) {
                                eprintln!("Cleanup warning: failed to remove incomplete asset folder {}: {:?}", download_directory_full_path.display(), err);
                            }
                            utils::emit_event(job_id.as_deref(), "cancelled", "Job cancelled", None, None);
                            if let Some(ref j) = job_id { utils::clear_cancel(j); }
                            return Err(HttpResponse::Ok().body("cancelled"));
                        }
                        eprintln!("Download failed from {}: {:?}", url, e);
                        continue;
                    }
                }
            }
        }
    }

    utils::emit_event(job_id.as_deref(), "download:error", "Unable to download asset from any distribution point", None, None);
    Ok(HttpResponse::InternalServerError().body("Unable to download asset from any distribution point"))
}

fn update_fab_cache_json(namespace: String, asset_id: String, artifact_id: String, ue_major_minor_version: Option<String>, title_folder: Option<String>, cache_path: &PathBuf) {
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

fn get_friendly_folder_name(asset_name: String) -> Option<String> {
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

async fn get_friendly_asset_name(namespace: &String, asset_id: &String, artifact_id: &String, mut epic_services: &mut EpicGames) -> String {
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

async fn epic_authenticate(epic_services: &mut EpicGames) {
    let auth_code = utils::get_auth_code();
    let _ = epic_services.auth_code(None, Some(auth_code)).await;
    let _ = epic_services.login().await;
    let _ = utils::save_user_details(&epic_services.user_details());
}

/// Lists Unreal Engine projects under a base directory by detecting folders containing a .uproject file.
///
/// Route:
/// - GET /list-unreal-projects
///
/// Query parameters:
/// - base: Optional override for the base directory. Defaults to $HOME/Documents/Unreal Projects.
///
/// Returns:
/// - 200 OK with JSON body: {
///     "base_directory": String,
///     "projects": [ { name, path, uproject_file }, ... ]
///   }
#[get("/list-unreal-projects")]
pub async fn list_unreal_projects(query: web::Query<std::collections::HashMap<String, String>>) -> impl Responder {
    // Optional query parameter: ?base=/custom/path
    let base_dir = query.get("base").map(|s| PathBuf::from(s)).unwrap_or_else(utils::default_unreal_projects_dir);
    
    let mut results: Vec<models::UnrealProjectInfo> = Vec::new();

    if base_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check for any .uproject file inside this directory (non-recursive)
                    if let Ok(sub) = fs::read_dir(&path) {
                        for f in sub.flatten() {
                            let p = f.path();
                            if p.is_file() {
                                if let Some(ext) = p.extension() {
                                    if ext == "uproject" {
                                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                                        // Try to read EngineAssociation from .uproject to determine UE version
                                        let mut engine_version = String::new();
                                        if let Ok(mut f) = fs::File::open(&p) {
                                            let mut buf = String::new();
                                            if f.read_to_string(&mut buf).is_ok() {
                                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&buf) {
                                                    if let Some(assoc) = v.get("EngineAssociation").and_then(|x| x.as_str()) {
                                                        if let Some(mm) = crate::utils::resolve_engine_association_to_mm(assoc) {
                                                            engine_version = mm;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        let info = models::UnrealProjectInfo {
                                            name,
                                            path: path.to_string_lossy().to_string(),
                                            uproject_file: p.to_string_lossy().to_string(),
                                            engine_version,
                                        };
                                        results.push(info);
                                        break; // one .uproject is enough to mark the directory as a project
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort by name for stable UI
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let response = models::UnrealProjectsResponse {
        base_directory: base_dir.to_string_lossy().to_string(),
        projects: results,
    };

    HttpResponse::Ok().json(response)
}


/// Lists installed Unreal Engine directories and attempts to determine their version and editor binary.
///
/// Route:
/// - GET /list-unreal-engines
///
/// Query parameters:
/// - base: Optional base directory containing engine folders. Defaults to $HOME/UnrealEngines.
///
/// Notes:
/// - Version is read from Engine/Build/Build.version when available; otherwise parsed heuristically from folder name.
/// - Editor path detection currently targets Linux layouts (Engine/Binaries/Linux/UnrealEditor or UE4Editor).
#[get("/list-unreal-engines")]
pub async fn list_unreal_engines(query: web::Query<std::collections::HashMap<String, String>>) -> impl Responder {
    let base_dir = query.get("base").map(|s| PathBuf::from(s)).unwrap_or_else(utils::default_unreal_engines_dir);

    let mut engines: Vec<models::UnrealEngineInfo> = Vec::new();
    if base_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Heuristic: consider any directory that has Engine/Binaries
                    if path.join("Engine").join("Binaries").is_dir() {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        let version = utils::read_build_version(&path)
                            .or_else(|| utils::parse_version_from_name(&name))
                            .unwrap_or_else(|| "unknown".to_string());
                        let editor_path = utils::find_editor_binary(&path).map(|p| p.to_string_lossy().to_string());
                        engines.push(models::UnrealEngineInfo {
                            name,
                            version,
                            path: path.to_string_lossy().to_string(),
                            editor_path,
                        });
                    }
                }
            }
        }
    }

    // Sort by version then name
    engines.sort_by(|a, b| a.version.cmp(&b.version).then(a.name.cmp(&b.name)));

    let resp = models::UnrealEnginesResponse {
        base_directory: base_dir.to_string_lossy().to_string(),
        engines,
    };

    HttpResponse::Ok().json(resp)
}


/// Launches Unreal Editor for a given project using a specified engine version.
///
/// Route:
/// - GET /open-unreal-project
///
/// Query parameters:
/// - project: Name of the project folder, a project directory path, or a .uproject file path.
/// - version: Engine version to use (e.g., 5.3 or 5.3.2). Exact match is preferred; prefix match is accepted.
/// - engine_base: Optional base directory to search for engines (defaults to $HOME/UnrealEngines).
/// - projects_base: Optional base directory containing UE projects when using a project name (defaults to $HOME/Documents/Unreal Projects).
///
/// Required fields: project, version. Optional: engine_base, projects_base.
///
/// Example requests:
/// - Using only the project name (uses default projects_base):
///   curl -G "http://127.0.0.1:8080/open-unreal-project" \
///        --data-urlencode "project=MyGame" \
///        --data-urlencode "version=5.3.2"
/// - Project dir + version, using default engine base:
///   curl -G "http://127.0.0.1:8080/open-unreal-project" \
///        --data-urlencode "project=$HOME/Documents/Unreal Projects/MyGame" \
///        --data-urlencode "version=5.3.2"
/// - Explicit .uproject path and custom engines/projects base directories:
///   curl -G "http://127.0.0.1:8080/open-unreal-project" \
///        --data-urlencode "project=$HOME/Documents/Unreal Projects/MyGame/MyGame.uproject" \
///        --data-urlencode "version=5.3" \
///        --data-urlencode "engine_base=$HOME/UnrealEngines" \
///        --data-urlencode "projects_base=$HOME/Documents/Unreal Projects"
///
/// URL-encoded form (for browsers or programmatic use):
///   /open-unreal-project?project=MyGame&version=5.3.2
///
/// Returns:
/// - 200 OK with JSON describing the launch when the editor was spawned.
/// - 4xx/5xx with JSON message explaining the error otherwise.
#[get("/open-unreal-project")]
pub async fn open_unreal_project(query: web::Query<std::collections::HashMap<String, String>>) -> impl Responder {
    // Accept either a full path/dir in `project` or a bare project name (folder name)
    let raw_project = match query.get("project") {
        Some(p) => p.clone(),
        None => {
            return HttpResponse::BadRequest().body("Missing required query parameter: project (name, path to .uproject, or project dir)");
        }
    };
    let version_param = match query.get("version") {
        Some(v) => v.clone(),
        None => {
            return HttpResponse::BadRequest().body("Missing required query parameter: version (e.g., 5.3.2 or 5.3)");
        }
    };
    let engine_base = query.get("engine_base").map(|s| PathBuf::from(s)).unwrap_or_else(utils::default_unreal_engines_dir);
    let projects_base = query
        .get("projects_base")
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(utils::default_unreal_projects_dir);
    println!("Project Base: {}", projects_base.to_string_lossy());
    println!("Raw Project: {}", raw_project);
    println!("Engine Base: {}", engine_base.to_string_lossy());
    println!("Version: {}", version_param);

    // First try to resolve as path/dir; if that fails, treat `raw_project` as a project name
    let project_path = match utils::resolve_project_path(&raw_project) {
        Some(p) => {
            println!("Resolve Project Path: {}", p.to_string_lossy());
            Some(p)
        },
        None => {
            // Interpret as a name: search projects_base/<name> for a .uproject file
            let candidate_dir = projects_base.join(&raw_project);
            println!("Candidate Dir: {}", candidate_dir.to_string_lossy());
            if candidate_dir.is_dir() {
                // Find the first .uproject file in that folder
                if let Ok(entries) = fs::read_dir(&candidate_dir) {
                    let mut found: Option<PathBuf> = None;
                    for e in entries.flatten() {
                        let fp = e.path();
                        if fp.is_file() {
                            if let Some(ext) = fp.extension() { if ext == "uproject" { found = Some(fp); break; } }
                        }
                    }
                    found
                } else { None }
            } else {
                None
            }
        }
    };

    let project_path = match project_path {
        Some(p) => {
            println!("Using project: {}", p.to_string_lossy());
            p
        },
        None => {
            return HttpResponse::BadRequest().body("Project not found by path or name, or no .uproject in directory");
        }
    };

    // Discover engines
    let mut engines: Vec<models::UnrealEngineInfo> = Vec::new();
    if engine_base.is_dir() {
        if let Ok(entries) = fs::read_dir(&engine_base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.join("Engine").join("Binaries").is_dir() {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        let version = utils::read_build_version(&path)
                            .or_else(|| utils::parse_version_from_name(&name))
                            .unwrap_or_else(|| "unknown".to_string());
                        let editor_path = utils::find_editor_binary(&path).map(|p| p.to_string_lossy().to_string());
                        engines.push(models::UnrealEngineInfo { name, version, path: path.to_string_lossy().to_string(), editor_path });
                    }
                }
            }
        }
    }

    if engines.is_empty() {
        return HttpResponse::NotFound().body("No Unreal Engine installations found in engine_base");
    }

    let chosen = match utils::pick_engine_for_version(&engines, &version_param) {
        Some(e) => e,
        None => {
            return HttpResponse::NotFound().body("Requested version not found among discovered engines");
        }
    };

    let editor_path = match &chosen.editor_path {
        Some(p) => PathBuf::from(p),
        None => return HttpResponse::NotFound().body("Engine found but Editor binary not located"),
    };
    println!("Using editor: {}", editor_path.to_string_lossy());

    // Spawn the editor without waiting for it to exit
    let spawn_res = std::process::Command::new(&editor_path)
        .arg(&project_path)
        .spawn();
    println!("Spawn Result: {:?}", spawn_res);

    match spawn_res {
        Ok(_child) => {
            let resp = models::OpenProjectResponse {
                launched: true,
                engine_name: Some(chosen.name.clone()),
                engine_version: Some(chosen.version.clone()),
                editor_path: Some(editor_path.to_string_lossy().to_string()),
                project: project_path.to_string_lossy().to_string(),
                message: "Launched Unreal Editor".to_string(),
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => {
            let resp = models::OpenProjectResponse {
                launched: false,
                engine_name: Some(chosen.name.clone()),
                engine_version: Some(chosen.version.clone()),
                editor_path: Some(editor_path.to_string_lossy().to_string()),
                project: project_path.to_string_lossy().to_string(),
                message: format!("Failed to launch editor: {}", e),
            };
            HttpResponse::InternalServerError().json(resp)
        }
    }
}

/// Import a previously downloaded asset into a UE project by copying its Content.
///
/// Route:
/// - POST /import-asset
///
/// JSON body fields:
/// - asset_name: String — The asset folder name under downloads/ (e.g., "Industry Props Pack 6"). Required.
/// - project: String — Project identifier. Accepts one of:
///   - Bare project folder name under the default projects dir (e.g., "MyGame").
///   - A project directory path (e.g., "$HOME/Documents/Unreal Projects/MyGame").
///   - A direct path to a .uproject file (e.g., "/path/to/MyGame.uproject"). Required.
/// - target_subdir: Optional<String> — Subfolder inside Project/Content to copy into (e.g., "Imported/Industry"). Optional.
/// - overwrite: Optional<bool> — When true, overwrite existing files; when false, keep existing files and count them as skipped. Default false.
///
/// Behavior:
/// - Copies all files from downloads/<asset_name>/data/Content into <Project>/Content (or the provided target_subdir).
/// - Creates missing directories as needed.
/// - Skips existing files unless overwrite=true.
/// - Returns counts for files copied and skipped, along with timing information.
///
/// Returns:
/// - 200 OK with JSON { ok, message, files_copied, files_skipped, source, destination, elapsed_ms } on success.
/// - 400 Bad Request if required fields are missing or the project cannot be resolved.
/// - 404 Not Found if the source Content folder for the asset does not exist.
/// - 500 Internal Server Error on copy failures.
///
/// Example requests:
/// - Basic import using project name (defaults to $HOME/Documents/Unreal Projects):
///   curl -X POST http://127.0.0.1:8080/import-asset \
///        -H "Content-Type: application/json" \
///        -d '{"asset_name":"Industry Props Pack 6","project":"MyGame"}'
/// - Import into a subfolder and overwrite existing files:
///   curl -X POST http://127.0.0.1:8080/import-asset \
///        -H "Content-Type: application/json" \
///        -d '{"asset_name":"Industry Props Pack 6","project":"MyGame","target_subdir":"Imported/Industry","overwrite":true}'
/// - Using an explicit .uproject path:
///   curl -X POST http://127.0.0.1:8080/import-asset \
///        -H "Content-Type: application/json" \
///        -d '{"asset_name":"Industry Props Pack 6","project":"$HOME/Documents/Unreal Projects/MyGame/MyGame.uproject"}'
#[post("/import-asset")]
pub async fn import_asset(body: web::Json<models::ImportAssetRequest>) -> impl Responder {
    let req = body.into_inner();
    let job_id = req.job_id.clone();
    utils::emit_event(job_id.as_deref(), "import:start", format!("Importing '{}'", req.asset_name), Some(0.0), None);

    // Determine downloads base (same logic as create_unreal_project)
    let mut downloads_base = PathBuf::from("downloads");
    if !downloads_base.exists() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let alt = exe_dir.join("downloads");
                if alt.exists() { downloads_base = alt; }
            }
        }
    }

    // If Fab identifiers are provided, run the exact same download process first
    if let (Some(namespace), Some(asset_id), Some(artifact_id)) = (req.namespace.clone(), req.asset_id.clone(), req.artifact_id.clone()) {
        // Forward jobId and ue parameters to the download handler
        let mut q: HashMap<String, String> = HashMap::new();
        if let Some(ref j) = job_id { q.insert("jobId".to_string(), j.clone()); }
        if let Some(ref ue) = req.ue { if !ue.trim().is_empty() { q.insert("ue".to_string(), ue.trim().to_string()); } }

        let path = web::Path::from((namespace.clone(), asset_id.clone(), artifact_id.clone()));
        let query: Query<HashMap<String, String>> = web::Query(q);
        match download_asset_handler(path, query).await {
            // Success/cancel paths in handler return Err(HttpResponse), inspect status
            Err(resp) => {
                if !resp.status().is_success() {
                    // Bubble up download error
                    return resp;
                }
                // If the job was cancelled, don't proceed to import
                if utils::is_cancelled(job_id.as_deref()) {
                    if let Some(ref j) = job_id { utils::clear_cancel(j); }
                    return HttpResponse::Ok().body("cancelled");
                }
                // Otherwise continue to import using the folder naming used by the downloader
                // Compute the folder name the same way as download_asset_handler
                let mut epic_services = utils::create_epic_games_services();
                if !utils::try_cached_login(&mut epic_services).await {
                    epic_authenticate(&mut epic_services).await;
                }
                let friendly = get_friendly_asset_name(&namespace, &asset_id, &artifact_id, &mut epic_services).await;
                let title_folder = get_friendly_folder_name(friendly);
                let mut computed_asset_dir = downloads_base.join(title_folder.unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id)));
                if let Some(ref ue) = req.ue { if !ue.trim().is_empty() { computed_asset_dir = computed_asset_dir.join(ue.trim()); } }
                // Prefer computed dir; if missing, fallback to provided asset_name resolution below
                // by storing this path for later if it exists
                if computed_asset_dir.exists() {
                    // Use this computed dir by setting a marker variable via shadowing later
                    // We'll pass through to common import logic using this path
                    // To do so, stash it in a mutable Option and use if present
                    // We'll proceed after the general preflight below
                    // Place into a thread-local compatible variable scope
                    // Continue to common path with computed_asset_dir
                    // To avoid duplication, jump to final copy section after preparing dest
                    // But for clarity, we'll fall through and let the preflight use this path
                }
            }
            // Handler returns Ok(HttpResponse) only on fatal failure paths (e.g., all dist points failed)
            Ok(resp) => {
                return resp;
            }
        }
    }

    // Resolve source: downloads/<asset_name>/data/Content, with smarter discovery:
    // 1) If Fab IDs were provided, try the computed folder name first (title or namespace-asset-artifact)
    // 2) Otherwise, use the provided asset_name with case-insensitive match
    let safe_name = req.asset_name.trim();
    if safe_name.is_empty() {
        return HttpResponse::BadRequest().body("asset_name is required");
    }

    let mut asset_dir: PathBuf;
    if let (Some(namespace), Some(asset_id), Some(artifact_id)) = (req.namespace.clone(), req.asset_id.clone(), req.artifact_id.clone()) {
        // Recompute expected folder name like the downloader
        let mut epic_services = utils::create_epic_games_services();
        if !utils::try_cached_login(&mut epic_services).await {
            epic_authenticate(&mut epic_services).await;
        }
        let friendly = get_friendly_asset_name(&namespace, &asset_id, &artifact_id, &mut epic_services).await;
        let title_folder = get_friendly_folder_name(friendly);
        let mut computed = downloads_base.join(title_folder.unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id)));
        if let Some(ref ue) = req.ue { if !ue.trim().is_empty() { computed = computed.join(ue.trim()); } }
        asset_dir = computed;
    } else {
        asset_dir = downloads_base.join(safe_name);
        if !asset_dir.exists() {
            if downloads_base.is_dir() {
                if let Ok(entries) = fs::read_dir(&downloads_base) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                                if fname.eq_ignore_ascii_case(safe_name) { asset_dir = p; break; }
                            }
                        }
                    }
                }
            }
        }
    }

    // Require that the asset exists locally now
    if !asset_dir.exists() {
        return HttpResponse::NotFound().body(format!("Asset folder not found under downloads (looked in {})", downloads_base.display()));
    }
    // If a completion marker is used by downloads, ensure it's complete as well
    if !utils::is_download_complete(&asset_dir) {
        return HttpResponse::NotFound().body("Asset is not fully downloaded. Please download it first via /download-asset.");
    }
    // Locate the source Content folder. Assets may place it at different depths (e.g., data/Content or data/Engine/Plugins/Marketplace/.../content)
    let data_dir = asset_dir.join("data");
    let mut src_content = data_dir.join("Content");
    if !src_content.is_dir() {
        // Try lowercase variant directly under data/
        let alt = data_dir.join("content");
        if alt.is_dir() {
            src_content = alt;
        } else {
            // Search recursively for a folder named Content/content (case-insensitive)
            let max_depth = 10usize;
            let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
            queue.push_back((data_dir.clone(), 0));
            let mut found: Option<PathBuf> = None;
            let mut found_marketplace: Option<PathBuf> = None;
            'bfs: while let Some((dir, depth)) = queue.pop_front() {
                if depth > max_depth { continue; }
                if let Ok(entries) = fs::read_dir(&dir) {
                    for ent in entries.flatten() {
                        let p = ent.path();
                        if p.is_dir() {
                            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                                if name.eq_ignore_ascii_case("Content") {
                                    let lower = p.to_string_lossy().to_lowercase();
                                    if lower.contains("plugins/marketplace") {
                                        found_marketplace = Some(p.clone());
                                        break 'bfs;
                                    }
                                    if found.is_none() { found = Some(p.clone()); }
                                }
                            }
                            queue.push_back((p, depth + 1));
                        }
                    }
                }
            }
            if let Some(p) = found_marketplace.or(found) {
                src_content = p;
            } else {
                return HttpResponse::NotFound().body(format!("Source Content folder not found under {}", data_dir.display()));
            }
        }
    }

    // Resolve project directory and destination Content
    let project_dir = match utils::resolve_project_dir_from_param(&req.project) {
        Some(p) => p,
        None => return HttpResponse::BadRequest().body("Project could not be resolved to a valid Unreal project"),
    };
    let mut dest_content = project_dir.join("Content");
    if let Some(sub) = &req.target_subdir {
        let trimmed = sub.trim_matches(['/', '\\']);
        if !trimmed.is_empty() {
            dest_content = dest_content.join(trimmed);
        }
    }
    // Always create an asset-named subfolder inside the project's Content and copy into it.
    // Use a friendly, filesystem-safe folder name derived from the requested asset_name.
    let asset_folder_name = get_friendly_folder_name(req.asset_name.clone()).unwrap_or_else(|| req.asset_name.clone());
    let dest_content = dest_content.join(asset_folder_name);

    let overwrite = req.overwrite.unwrap_or(false);
    let started = Instant::now();
    utils::emit_event(job_id.as_deref(), "import:copying", format!("Copying files into {}", dest_content.display()), Some(0.0), None);
    match utils::copy_dir_recursive_with_progress(&src_content, &dest_content, overwrite, job_id.as_deref(), "import:copying") {
        Ok((copied, skipped)) => {
            utils::emit_event(job_id.as_deref(), "import:complete", format!("Imported '{}'", req.asset_name.trim()), Some(100.0), None);
            let resp = models::ImportAssetResponse {
                ok: true,
                message: format!("Imported into project at {}", project_dir.display()),
                files_copied: copied,
                files_skipped: skipped,
                source: src_content.to_string_lossy().to_string(),
                destination: dest_content.to_string_lossy().to_string(),
                elapsed_ms: started.elapsed().as_millis(),
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => {
            utils::emit_event(job_id.as_deref(), "import:error", format!("Failed to import: {}", e), None, None);
            let resp = models::ImportAssetResponse {
                ok: false,
                message: format!("Failed to import: {}", e),
                files_copied: 0,
                files_skipped: 0,
                source: src_content.to_string_lossy().to_string(),
                destination: dest_content.to_string_lossy().to_string(),
                elapsed_ms: started.elapsed().as_millis(),
            };
            HttpResponse::InternalServerError().json(resp)
        }
    }
}


/// Simple health check endpoint to verify the service is running.
///
/// Route:
/// - GET /health
///
/// Returns:
/// - 200 OK with body "OK".
#[get("/health")]
pub async fn health() -> HttpResponse {
    HttpResponse::Ok().body("OK")
}

/// Welcome endpoint providing quick pointers to common routes.
///
/// Route:
/// - GET /
///
/// Returns:
/// - 200 OK with a short informational message.
#[get("/")]
pub async fn root() -> HttpResponse {
    HttpResponse::Ok().body(
        "egs_client is running. Try /health, /get-fab-list, or /refresh-fab-list."
    )
}

/// Returns the backend app version derived from Cargo.toml at compile time.
///
/// Route:
/// - GET /version
///
/// Returns JSON: { "version": "x.y.z", "name": "<package name>" }
#[get("/version")]
pub async fn get_version() -> HttpResponse {
    let ver = env!("CARGO_PKG_VERSION");
    let name = env!("CARGO_PKG_NAME");
    let body = serde_json::json!({
        "version": ver,
        "name": name,
    });
    HttpResponse::Ok().json(body)
}

/// Set/override the Unreal Engine version (EngineAssociation) in a .uproject.
///
/// Route:
/// - POST /set-unreal-project-version
///
/// JSON body:
/// - project: Name, directory, or path to a .uproject
/// - version: UE version like "5.6" (also accepts "5.6.1" or "UE_5.6")
#[post("/set-unreal-project-version")]
pub async fn set_unreal_project_version(body: web::Json<models::SetProjectEngineRequest>) -> impl Responder {
    let req = body.into_inner();
    let mut s = req.version.trim().to_string();
    if let Some(rest) = s.strip_prefix("UE_") { s = rest.to_string(); }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 { return HttpResponse::BadRequest().body("version must be like 5.6 or UE_5.6 (patch allowed)"); }
    let major = parts[0].trim();
    let minor = parts[1].trim();
    if major.is_empty() || minor.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) || !minor.chars().all(|c| c.is_ascii_digit()) {
        return HttpResponse::BadRequest().body("version must be like 5.6 or UE_5.6 (patch allowed)");
    }
    let mm = format!("{}.{}", major, minor);

    // Resolve .uproject path
    let mut uproject_path: Option<PathBuf> = utils::resolve_project_path(&req.project);
    if uproject_path.is_none() {
        if let Some(project_dir) = utils::resolve_project_dir_from_param(&req.project) {
            // Find a .uproject inside
            if let Ok(entries) = fs::read_dir(&project_dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_file() && p.extension().map_or(false, |ext| ext == "uproject") {
                        uproject_path = Some(p);
                        break;
                    }
                }
            }
        }
    }
    let uproject = match uproject_path {
        Some(p) => p,
        None => return HttpResponse::BadRequest().body("Project could not be resolved to a .uproject"),
    };

    // Read, modify, write JSON
    let content = match fs::read_to_string(&uproject) {
        Ok(s) => s,
        Err(e) => return HttpResponse::InternalServerError().body(format!("Failed to read .uproject: {}", e)),
    };
    let mut v: serde_json::Value = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(e) => return HttpResponse::BadRequest().body(format!(".uproject is not valid JSON: {}", e)),
    };
    // Set EngineAssociation to normalized major.minor
    if let Some(obj) = v.as_object_mut() {
        obj.insert("EngineAssociation".to_string(), serde_json::Value::String(mm.clone()))
            .is_some();
    } else {
        return HttpResponse::BadRequest().body(".uproject JSON is not an object");
    }
    let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
    if let Err(e) = fs::write(&uproject, pretty) {
        return HttpResponse::InternalServerError().body(format!("Failed to write .uproject: {}", e));
    }

    HttpResponse::Ok().json(models::SimpleResponse { ok: true, message: format!("Set EngineAssociation to {}", mm) })
}

/// Creates a new Unreal Engine project from a template/sample `.uproject` using UnrealEditor `-CopyProject`.
///
/// Route:
/// - POST /create-unreal-project
///
/// JSON body fields:
/// - engine_path: Optional<String> — Path to a specific Unreal Engine installation. If omitted, the server will
///   search the default engines directory (see list-unreal-engines) and pick the latest-looking one. Optional.
/// - template_project: String — Path to a template/sample `.uproject`, or a directory containing one. Required unless `asset_name` is provided.
/// - asset_name: Optional<String> — Convenience: name of a downloaded sample under `downloads/` (e.g., "Stack O Bot").
///   When provided and `template_project` is empty, the server searches `downloads/<asset_name>/` recursively for a `.uproject` to use as the template.
/// - output_dir: String — Directory where the new project folder will be created. Required.
/// - project_name: String — Name of the new project folder to create under `output_dir`. Required.
/// - project_type: Optional<String> — "bp" for Blueprint-only (adds -NoCompile to skip compiling C++ targets on open) or "cpp". Default: "bp".
/// - open_after_create: Optional<bool> — When true, the server will launch Unreal Editor to open the created project after copying. Default: false.
/// - dry_run: Optional<bool> — When true, returns the constructed command without executing UnrealEditor. Optional.
///
/// Behavior:
/// - Locates UnrealEditor under the given engine_path or auto-discovers from the default engines directory.
/// - Resolves the template `.uproject` (if a directory is provided, it finds the first `.uproject` inside).
/// - Ensures `output_dir` exists and computes `<output_dir>/<project_name>` as the destination.
/// - Copies the template project directory to the new location (excluding Binaries/DerivedDataCache/Intermediate/Saved/etc.).
/// - Builds an "open" command for UnrealEditor but does not run it unless `open_after_create=true`.
/// - If `dry_run=true`, returns the command preview without launching the editor.
/// - Response is returned immediately after project creation (and spawn when applicable), without waiting for Unreal Editor to exit.
///
/// Returns:
/// - 200 OK with JSON { ok: true, message, command, project_path } on success or dry-run.
/// - 400 Bad Request if inputs are invalid or UnrealEditor cannot be located.
/// - 500 Internal Server Error only for copy/creation failures (opening the editor is optional; failures are reported in message with ok=true).
///
/// Example (dry run):
/// - Direct template path:
///   curl -s -X POST http://127.0.0.1:8080/create-unreal-project \
///        -H "Content-Type: application/json" \
///        -d '{
///              "engine_path": null,
///              "template_project": "/path/to/Sample/Sample.uproject",
///              "output_dir": "'$HOME/Documents/Unreal Projects'",
///              "project_name": "MyNewGame",
///              "project_type": "bp",
///              "dry_run": true
///            }' | jq
/// - Using downloads and Stack O Bot by name:
///   curl -s -X POST http://127.0.0.1:8080/create-unreal-project \
///        -H "Content-Type: application/json" \
///        -d '{
///              "asset_name": "Stack O Bot",
///              "output_dir": "'$HOME/Documents/Unreal Projects'",
///              "project_name": "MyStackOBotCopy",
///              "project_type": "bp",
///              "dry_run": true
///            }' | jq
#[post("/create-unreal-project")]
pub async fn create_unreal_project(body: web::Json<models::CreateUnrealProjectRequest>) -> impl Responder {
    let req = body.into_inner();
    let job_id = req.job_id.clone();
    utils::emit_event(job_id.as_deref(), "create:start", format!("Creating project {}", req.project_name), None, None);

    // If Fab identifiers are provided, reuse the same download flow as /import-asset to ensure the sample/template is present
    if let (Some(namespace), Some(asset_id), Some(artifact_id)) = (req.namespace.clone(), req.asset_id.clone(), req.artifact_id.clone()) {
        let mut q: HashMap<String, String> = HashMap::new();
        if let Some(ref j) = job_id { q.insert("jobId".to_string(), j.clone()); }
        if let Some(ref ue) = req.ue { if !ue.trim().is_empty() { q.insert("ue".to_string(), ue.trim().to_string()); } }
        let path = web::Path::from((namespace.clone(), asset_id.clone(), artifact_id.clone()));
        let query: Query<HashMap<String, String>> = web::Query(q);
        match download_asset_handler(path, query).await {
            Err(resp) => {
                if !resp.status().is_success() { return resp; }
                if utils::is_cancelled(job_id.as_deref()) { if let Some(ref j) = job_id { utils::clear_cancel(j); } return HttpResponse::Ok().body("cancelled"); }
            }
            Ok(resp) => { return resp; }
        }
    }

    // Validate inputs
    let template_empty = req.template_project.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true);
    let asset_empty = req.asset_name.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true);
    if template_empty && asset_empty {
        return HttpResponse::BadRequest().body("Provide either template_project (path/dir) or asset_name (under downloads/)");
    }
    if req.output_dir.trim().is_empty() { return HttpResponse::BadRequest().body("output_dir is required"); }
    if req.project_name.trim().is_empty() { return HttpResponse::BadRequest().body("project_name is required"); }

    let project_type = req.project_type.unwrap_or_else(|| "bp".to_string()).to_lowercase();
    if project_type != "bp" && project_type != "cpp" {
        return HttpResponse::BadRequest().body("project_type must be 'bp' or 'cpp'");
    }

    // Resolve engine path: prefer explicit engine_path; else if ue provided, pick matching engine; else pick latest
    let engine_path = if let Some(p) = req.engine_path.clone() { PathBuf::from(p) } else if let Some(ue) = req.ue.clone() {
        let base = utils::default_unreal_engines_dir();
        let mut engines: Vec<models::UnrealEngineInfo> = Vec::new();
        if base.is_dir() {
            if let Ok(entries) = fs::read_dir(&base) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() && p.join("Engine").join("Binaries").exists() {
                        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        let version = utils::read_build_version(&p)
                            .or_else(|| utils::parse_version_from_name(&name))
                            .unwrap_or_else(|| "unknown".to_string());
                        let editor_path = utils::find_editor_binary(&p).map(|pp| pp.to_string_lossy().to_string());
                        engines.push(models::UnrealEngineInfo { name, version, path: p.to_string_lossy().to_string(), editor_path });
                    }
                }
            }
        }
        match utils::pick_engine_for_version(&engines, &ue) {
            Some(info) => PathBuf::from(info.path.clone()),
            None => return HttpResponse::NotFound().body("Requested UE version not found among discovered engines"),
        }
    } else {
        let base = utils::default_unreal_engines_dir();
        // pick the first engine when sorted descending by version string
        let mut engines: Vec<PathBuf> = Vec::new();
        if base.is_dir() { if let Ok(entries) = fs::read_dir(&base) { for e in entries.flatten() { let p = e.path(); if p.is_dir() && p.join("Engine").exists() { engines.push(p); } } } }
        if engines.is_empty() { return HttpResponse::BadRequest().body("engine_path not provided and no engines found in default location"); }
        engines.sort_by(|a,b| b.file_name().unwrap_or_default().cmp(a.file_name().unwrap_or_default()));
        engines[0].clone()
    };

    // Locate UnrealEditor binary (Linux path). Add Windows/macOS variants as needed.
    let editor_bin_candidates = [
        engine_path.join("Engine/Binaries/Linux/UnrealEditor"),
        engine_path.join("Engine/Binaries/Linux/UnrealEditor.exe"), // in case of WSL path
        engine_path.join("Engine/Binaries/Win64/UnrealEditor.exe"),
        engine_path.join("Engine/Binaries/Mac/UnrealEditor.app/Contents/MacOS/UnrealEditor"),
    ];
    let editor_path = editor_bin_candidates.iter().find(|p| p.exists()).cloned();
    let editor_path = match editor_path { Some(p) => p, None => return HttpResponse::BadRequest().body("Unable to locate UnrealEditor binary under engine_path") };

    // Resolve template path from either template_project or asset_name under downloads/
    fn trim_quotes_and_expand_home(s: &str) -> String {
        let mut t = s.trim().to_string();
        // Trim matching leading/trailing single or double quotes
        if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
            t = t[1..t.len()-1].to_string();
        }
        // Expand $HOME and leading ~
        if let Ok(home) = std::env::var("HOME") {
            if t.starts_with("~/") { t = t.replacen("~", &home, 1); }
            if t.contains("$HOME") { t = t.replace("$HOME", &home); }
        }
        t
    }

    // Breadth-first search for .uproject, preferring shallow matches, skipping non-project data dirs
    fn find_uproject_bfs(start: &Path, max_depth: usize) -> Option<PathBuf> {
        use std::collections::VecDeque;
        if max_depth == 0 { return None; }
        let mut q: VecDeque<(PathBuf, usize)> = VecDeque::new();
        q.push_back((start.to_path_buf(), 0));
        while let Some((dir, depth)) = q.pop_front() {
            if dir.is_file() {
                if dir.extension().and_then(|s| s.to_str()) == Some("uproject") { return Some(dir); }
                continue;
            }
            if !dir.is_dir() { continue; }
            // 1) Check this directory for any .uproject files first (non-recursive)
            if let Ok(entries) = fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_file() {
                        if p.extension().and_then(|s| s.to_str()) == Some("uproject") { return Some(p); }
                    }
                }
            }
            if depth >= max_depth { continue; }
            // 2) Enqueue child directories, skipping well-known non-project dirs
            if let Ok(entries) = fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                            let lname = name.to_ascii_lowercase();
                            if lname == "content" || lname == ".git" || lname == ".svn" { continue; }
                        }
                        q.push_back((p, depth + 1));
                    }
                }
            }
        }
        None
    }

    let template_path: Option<PathBuf> = if let Some(tp) = req.template_project.as_deref() {
        let tp = tp.trim();
        if tp.is_empty() { None } else {
            let candidate = PathBuf::from(trim_quotes_and_expand_home(tp));
            if candidate.is_dir() { find_uproject_bfs(&candidate, 5) } else { Some(candidate) }
        }
    } else if let Some(name) = req.asset_name.as_ref() {
        // Resolve downloads base robustly: try ./downloads relative to CWD, else relative to executable dir
        let mut downloads_base = PathBuf::from("downloads");
        if !downloads_base.exists() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_dir) = exe.parent() {
                    let alt = exe_dir.join("downloads");
                    if alt.exists() { downloads_base = alt; }
                }
            }
        }
        // If the exact asset_name folder doesn't exist, try case-insensitive match among children
        let mut asset_dir = downloads_base.join(name);
        if !asset_dir.exists() {
            if downloads_base.is_dir() {
                if let Ok(entries) = fs::read_dir(&downloads_base) {
                    for e in entries.flatten() {
                        let p = e.path();
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
        }
        // If missing or incomplete, attempt to (re)download by name first
        let needs_download = !asset_dir.exists() || !utils::is_download_complete(&asset_dir);
        if needs_download {
            utils::emit_event(job_id.as_deref(), "create:downloading", format!("Downloading '{}'", name), Some(0.0), None);
            match utils::ensure_asset_downloaded_by_name(name, job_id.as_deref(), "create:downloading").await {
                Ok(p) => { asset_dir = p; utils::emit_event(job_id.as_deref(), "create:downloading", format!("Downloaded '{}'", name), Some(100.0), None); },
                Err(err) => {
                    eprintln!("{}", err);
                    utils::emit_event(job_id.as_deref(), "create:error", format!("Failed to download '{}'", name), None, None);
                    return HttpResponse::NotFound().body(format!("{}", err));
                }
            }
        }
        // Log what base/asset dir we ended up with for diagnostics
        println!("Searching for .uproject under: {}", asset_dir.to_string_lossy());
        find_uproject_bfs(&asset_dir, 8)
    } else { None };

    let template_path = match template_path {
        Some(p) if p.extension().and_then(|s| s.to_str()) == Some("uproject") => {
            println!("Using template .uproject: {}", p.to_string_lossy());
            p
        },
        _ => return HttpResponse::BadRequest().body("Unable to resolve a .uproject from template_project/asset_name. Tips: ensure there is a .uproject inside the selected folder; if using asset_name, verify the asset exists under downloads/ (case-insensitive match is supported) and that the .uproject isn’t packaged deep inside nested 'data' or 'Content' folders."),
    };

    // Canonicalize template_path to an absolute path where possible
    let template_path = std::fs::canonicalize(&template_path)
        .unwrap_or_else(|_| std::env::current_dir().map(|cwd| cwd.join(&template_path)).unwrap_or(template_path));

    // Normalize and absolutize out_dir (ensure it exists before canonicalize)
    let out_dir = PathBuf::from(trim_quotes_and_expand_home(&req.output_dir));
    if !out_dir.exists() {
        if let Err(e) = fs::create_dir_all(&out_dir) {
            return HttpResponse::InternalServerError().body(format!("Failed to create output_dir: {}", e));
        }
    }
    let out_dir = std::fs::canonicalize(&out_dir)
        .unwrap_or_else(|_| std::env::current_dir().map(|cwd| cwd.join(&out_dir)).unwrap_or(out_dir));
    let new_project_dir = out_dir.join(&req.project_name);

    // Ensure destination project directory exists before copying
    if let Err(e) = fs::create_dir_all(&new_project_dir) {
        return HttpResponse::InternalServerError().body(format!("Failed to create new project directory: {}", e));
    }

    // Instead of using -CopyProject (not valid), copy the template project folder and open the new project.
    // Determine the template project root (directory that contains the .uproject)
    let template_dir = template_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Build list of exclusions
    let exclude_names = ["Binaries", "DerivedDataCache", "Intermediate", "Saved", ".git", ".svn", ".vs"]; 

    // Prepare dry-run summary string
    let mut actions: Vec<String> = Vec::new();
    actions.push(format!("Copy '{}' -> '{}' (excluding {:?})", template_dir.to_string_lossy(), new_project_dir.to_string_lossy(), exclude_names));

    // Perform copy with progress logging
    // Pre-count total files to be copied (respecting exclusions)
    let mut total_copy_files: usize = 0;
    for entry in walkdir::WalkDir::new(&template_dir).into_iter().filter_map(|e| e.ok()) {
        let src_path = entry.path();
        let rel = match src_path.strip_prefix(&template_dir) { Ok(r) => r, Err(_) => continue };
        if rel.as_os_str().is_empty() { continue; }
        if let Some(first) = rel.components().next() {
            use std::path::Component;
            let name = match first { Component::Normal(os) => os.to_string_lossy().to_string(), _ => String::new() };
            if exclude_names.iter().any(|ex| name.eq_ignore_ascii_case(ex)) { continue; }
        }
        if entry.file_type().is_file() { total_copy_files += 1; }
    }

    println!("[copy-start] {} -> {} ({} files, excluding {:?})",
        template_dir.to_string_lossy(), new_project_dir.to_string_lossy(), total_copy_files, exclude_names);
    utils::emit_event(job_id.as_deref(), "create:copying", format!("Creating new project at {}", new_project_dir.to_string_lossy()), Some(0.0), None);

    let mut copied_files = 0usize;
    let mut skipped_files = 0usize;
    let mut last_logged_percent: u32 = 0;
    let mut last_log_instant = Instant::now();

    let walker = walkdir::WalkDir::new(&template_dir).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        let src_path = entry.path();
        let rel = match src_path.strip_prefix(&template_dir) { Ok(r) => r, Err(_) => continue };
        if rel.as_os_str().is_empty() { continue; }
        // Skip excluded top-level dirs and their contents
        if let Some(first) = rel.components().next() {
            use std::path::Component;
            let name = match first { Component::Normal(os) => os.to_string_lossy().to_string(), _ => String::new() };
            if exclude_names.iter().any(|ex| name.eq_ignore_ascii_case(ex)) { skipped_files += 1; continue; }
        }
        let dst_path = new_project_dir.join(rel);
        if entry.file_type().is_dir() {
            if let Err(e) = fs::create_dir_all(&dst_path) { return HttpResponse::InternalServerError().body(format!("Failed to create dir {}: {}", dst_path.to_string_lossy(), e)); }
        } else if entry.file_type().is_file() {
            // If this is the template .uproject, we will rename its filename to match the new project
            let mut final_dst = dst_path.clone();
            if src_path.extension().and_then(|s| s.to_str()) == Some("uproject") {
                final_dst = new_project_dir.join(format!("{}.uproject", req.project_name));
            }
            if let Some(parent) = final_dst.parent() { if let Err(e) = fs::create_dir_all(parent) { return HttpResponse::InternalServerError().body(format!("Failed to create parent dir {}: {}", parent.to_string_lossy(), e)); } }
            if let Err(e) = fs::copy(src_path, &final_dst) { return HttpResponse::InternalServerError().body(format!("Failed to copy {} -> {}: {}", src_path.to_string_lossy(), final_dst.to_string_lossy(), e)); }
            copied_files += 1;

            if total_copy_files > 0 {
                let percent = ((copied_files as f64 / total_copy_files as f64) * 100.0).floor() as u32;
                if percent >= last_logged_percent + 5 || last_log_instant.elapsed().as_secs() >= 2 {
                    println!("[copy-progress] {}/{} ({}%) - {}", copied_files, total_copy_files, percent, rel.to_string_lossy());
                    last_logged_percent = percent;
                    last_log_instant = Instant::now();
                    utils::emit_event(job_id.as_deref(), "create:copying", format!("{} / {}", copied_files, total_copy_files), Some(percent as f32), None);
                }
            }
        } else if entry.file_type().is_symlink() {
            // Skip symlinks to avoid unexpected behavior
            skipped_files += 1;
        }
    }

    println!("[copy-finish] Copied {} files ({} skipped) to {}",
        copied_files, skipped_files, new_project_dir.to_string_lossy());
    utils::emit_event(job_id.as_deref(), "create:complete", format!("Project created at {}", new_project_dir.to_string_lossy()), Some(100.0), None);

    // Determine new .uproject path
    let new_uproject = new_project_dir.join(format!("{}.uproject", req.project_name));
    // If the rename logic didn’t find any .uproject to copy (rare), fall back to copying the same name
    if !new_uproject.exists() {
        let src_name = template_path.file_name().unwrap_or_default();
        let fallback = new_project_dir.join(src_name);
        if !fallback.exists() {
            if let Err(e) = fs::copy(&template_path, &fallback) { return HttpResponse::InternalServerError().body(format!("Failed to copy template .uproject: {}", e)); }
        }
    }

    // Optionally update project name in .uproject file if a Name field exists
    let target_uproject = if new_uproject.exists() { new_uproject.clone() } else { new_project_dir.join(template_path.file_name().unwrap_or_default()) };
    if let Ok(json_text) = fs::read_to_string(&target_uproject) {
        if json_text.contains("\"FileVersion\"") || json_text.contains("\"EngineAssociation\"") {
            // Best-effort: update the FriendlyName or DisplayedName if present
            let updated = json_text
                .replace("\"DisplayName\":\"", &format!("\"DisplayName\":\"{}", req.project_name))
                .replace("\"FriendlyName\":\"", &format!("\"FriendlyName\":\"{}", req.project_name));
            if updated != json_text {
                if let Err(e) = fs::write(&target_uproject, updated) { eprintln!("Warning: failed to update project name in .uproject: {}", e); }
            }
        }
    }

    // If a UE version was specified, set EngineAssociation in the created .uproject to its major.minor form
    if let Some(mut ue) = req.ue.clone() {
        ue = ue.trim().to_string();
        if ue.starts_with("UE_") { ue = ue[3..].to_string(); }
        let parts: Vec<&str> = ue.split('.').collect();
        if parts.len() >= 2 {
            let mm = format!("{}.{}", parts[0].trim(), parts[1].trim());
            if let Ok(text) = fs::read_to_string(&target_uproject) {
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(mut v) => {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("EngineAssociation".to_string(), serde_json::Value::String(mm.clone()));
                            if let Ok(pretty) = serde_json::to_string_pretty(&v) {
                                if let Err(e) = fs::write(&target_uproject, pretty) { eprintln!("Warning: failed to write EngineAssociation: {}", e); }
                            }
                        }
                    }
                    Err(e) => { eprintln!("Warning: .uproject JSON parse failed when setting EngineAssociation: {}", e); }
                }
            }
        }
    }

    // Build open command: UnrealEditor <NewProject.uproject> [-NoCompile]
    let mut cmd = std::process::Command::new(&editor_path);
    cmd.arg(&target_uproject);
    if project_type == "bp" { cmd.arg("-NoCompile"); }
    let command_preview = format!("{} {}{}",
        editor_path.to_string_lossy(),
        target_uproject.to_string_lossy(),
        if project_type == "bp" { " -NoCompile" } else { "" }
    );
    println!("UnrealEditor: {}", editor_path.to_string_lossy());
    println!("Open Command: {}", command_preview);

    if req.dry_run.unwrap_or(false) {
        actions.push(format!("Open with: {}", command_preview));
        let resp = models::CreateUnrealProjectResponse { ok: true, message: format!("Dry run: would copy {} files (skipped {}), then open project{}", copied_files, skipped_files, if req.open_after_create.unwrap_or(false) { " (open_after_create=true)" } else { "" }), command: actions.join(" | "), project_path: Some(new_project_dir.to_string_lossy().to_string()) };
        return HttpResponse::Ok().json(resp);
    }

    // Decide whether to open after create (default false)
    let open_after = req.open_after_create.unwrap_or(false);
    if open_after {
        match cmd.spawn() {
            Ok(_child) => {
                let resp = models::CreateUnrealProjectResponse {
                    ok: true,
                    message: format!("Project created ({} files, {} skipped). Unreal Editor is launching...", copied_files, skipped_files),
                    command: command_preview,
                    project_path: Some(new_project_dir.to_string_lossy().to_string()),
                };
                return HttpResponse::Ok().json(resp);
            }
            Err(e) => {
                let resp = models::CreateUnrealProjectResponse {
                    ok: true, // project created successfully; opening is optional
                    message: format!("Project created ({} files, {} skipped). Failed to launch UnrealEditor: {}", copied_files, skipped_files, e),
                    command: command_preview,
                    project_path: Some(new_project_dir.to_string_lossy().to_string()),
                };
                return HttpResponse::Ok().json(resp);
            }
        }
    } else {
        let resp = models::CreateUnrealProjectResponse {
            ok: true,
            message: format!("Project created ({} files, {} skipped). Not opening (open_after_create=false).", copied_files, skipped_files),
            command: command_preview,
            project_path: Some(new_project_dir.to_string_lossy().to_string()),
        };
        return HttpResponse::Ok().json(resp);
    }
}


/// Launches Unreal Editor for a given engine version (no project).
///
/// Route:
/// - GET /open-unreal-engine
///
/// Query parameters:
/// - version: Engine version to use (e.g., 5.3 or 5.3.2). Exact match is preferred; prefix match is accepted.
/// - engine_base: Optional base directory to search for engines (defaults to $HOME/UnrealEngines).
///
/// Returns:
/// - 200 OK with JSON describing the launch when the editor was spawned.
/// - 4xx/5xx with JSON message explaining the error otherwise.
#[get("/open-unreal-engine")]
pub async fn open_unreal_engine(query: web::Query<std::collections::HashMap<String, String>>) -> impl Responder {
    let version_param = match query.get("version") {
        Some(v) => v.clone(),
        None => {
            return HttpResponse::BadRequest().body("Missing required query parameter: version (e.g., 5.3.2 or 5.3)");
        }
    };
    let engine_base = query
        .get("engine_base")
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(utils::default_unreal_engines_dir);

    println!("Engine Base: {}", engine_base.to_string_lossy());
    println!("Version: {}", version_param);

    // Discover engines
    let mut engines: Vec<models::UnrealEngineInfo> = Vec::new();
    if engine_base.is_dir() {
        if let Ok(entries) = fs::read_dir(&engine_base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.join("Engine").join("Binaries").is_dir() {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        let version = utils::read_build_version(&path)
                            .or_else(|| utils::parse_version_from_name(&name))
                            .unwrap_or_else(|| "unknown".to_string());
                        let editor_path = utils::find_editor_binary(&path).map(|p| p.to_string_lossy().to_string());
                        engines.push(models::UnrealEngineInfo { name, version, path: path.to_string_lossy().to_string(), editor_path });
                    }
                }
            }
        }
    }

    if engines.is_empty() {
        return HttpResponse::NotFound().body("No Unreal Engine installations found in engine_base");
    }

    let chosen = match utils::pick_engine_for_version(&engines, &version_param) {
        Some(e) => e,
        None => {
            return HttpResponse::NotFound().body("Requested version not found among discovered engines");
        }
    };

    let editor_path = match &chosen.editor_path {
        Some(p) => PathBuf::from(p),
        None => return HttpResponse::NotFound().body("Engine found but Editor binary not located"),
    };

    println!("Using editor: {}", editor_path.to_string_lossy());

    // Spawn the editor without waiting for it to exit (no project argument)
    let spawn_res = std::process::Command::new(&editor_path).spawn();
    println!("Spawn Result: {:?}", spawn_res);

    match spawn_res {
        Ok(_child) => {
            let resp = models::OpenEngineResponse {
                launched: true,
                engine_name: Some(chosen.name.clone()),
                engine_version: Some(chosen.version.clone()),
                editor_path: Some(editor_path.to_string_lossy().to_string()),
                message: "Launched Unreal Editor".to_string(),
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => {
            let resp = models::OpenEngineResponse {
                launched: false,
                engine_name: Some(chosen.name.clone()),
                engine_version: Some(chosen.version.clone()),
                editor_path: Some(editor_path.to_string_lossy().to_string()),
                message: format!("Failed to launch editor: {}", e),
            };
            HttpResponse::InternalServerError().json(resp)
        }
    }
}


#[get("/config/paths")]
pub async fn get_paths_config() -> HttpResponse {
    let cfg = utils::load_paths_config();
    let status = models::PathsStatus {
        configured: cfg.clone(),
        effective_projects_dir: utils::default_unreal_projects_dir().to_string_lossy().to_string(),
        effective_engines_dir: utils::default_unreal_engines_dir().to_string_lossy().to_string(),
        effective_cache_dir: utils::default_cache_dir().to_string_lossy().to_string(),
        effective_downloads_dir: utils::default_downloads_dir().to_string_lossy().to_string(),
    };
    HttpResponse::Ok().json(status)
}


#[post("/config/paths")]
pub async fn set_paths_config(body: web::Json<models::PathsUpdate>) -> HttpResponse {
    let mut cfg = utils::load_paths_config();
    // Merge updates
    if let Some(p) = &body.projects_dir {
        cfg.projects_dir = Some(p.trim().to_string());
    }
    if let Some(e) = &body.engines_dir {
        cfg.engines_dir = Some(e.trim().to_string());
    }
    if let Some(c) = &body.cache_dir {
        cfg.cache_dir = Some(c.trim().to_string());
    }
    if let Some(d) = &body.downloads_dir {
        cfg.downloads_dir = Some(d.trim().to_string());
    }
    if let Err(e) = utils::save_paths_config(&cfg) {
        return HttpResponse::InternalServerError().body(format!("Failed to save config: {}", e));
    }
    let status = models::PathsStatus {
        configured: cfg.clone(),
        effective_projects_dir: utils::default_unreal_projects_dir().to_string_lossy().to_string(),
        effective_engines_dir: utils::default_unreal_engines_dir().to_string_lossy().to_string(),
        effective_cache_dir: utils::default_cache_dir().to_string_lossy().to_string(),
        effective_downloads_dir: utils::default_downloads_dir().to_string_lossy().to_string(),
    };
    HttpResponse::Ok().json(status)
}


#[post("/cancel-job")]
pub async fn cancel_job_endpoint(query: web::Query<std::collections::HashMap<String, String>>) -> HttpResponse {
    let job_id = query.get("jobId").cloned().or_else(|| query.get("job_id").cloned());
    if let Some(jid) = job_id {
        utils::cancel_job(&jid);
        utils::emit_event(Some(&jid), "cancelled", "Job cancelled", None, None);
        return HttpResponse::Ok().json(serde_json::json!({"ok": true, "message": "cancelled"}));
    }
    HttpResponse::BadRequest().body("missing jobId")
}



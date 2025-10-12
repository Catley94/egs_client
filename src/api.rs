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


#[post("/auth/complete")]
pub async fn auth_complete(body: web::Json<models::AuthCompleteRequest>) -> HttpResponse {
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
    match utils::download_asset_handler(path, query).await {
        Ok(value) => value,
        Err(value) => return value,
    }
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
/// - version: Optional engine version to use (e.g., 5.3 or 5.3.2). If omitted, the server reads EngineAssociation from the .uproject and picks the matching engine. Exact match is preferred; prefix match is accepted.
/// - engine_base: Optional base directory to search for engines (defaults to $HOME/UnrealEngines).
/// - projects_base: Optional base directory containing UE projects when using a project name (defaults to $HOME/Documents/Unreal Projects).
///
/// Required fields: project. Optional: version, engine_base, projects_base.
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
    let version_param_opt = query.get("version").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let engine_base = query.get("engine_base").map(|s| PathBuf::from(s)).unwrap_or_else(utils::default_unreal_engines_dir);
    let projects_base = query
        .get("projects_base")
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(utils::default_unreal_projects_dir);
    println!("Project Base: {}", projects_base.to_string_lossy());
    println!("Raw Project: {}", raw_project);
    println!("Engine Base: {}", engine_base.to_string_lossy());
    println!("Version (requested): {}", version_param_opt.clone().unwrap_or_else(|| "<auto> from .uproject".to_string()));

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

    // Determine requested version: either from query or from the project's EngineAssociation
    let requested_version = if let Some(v) = version_param_opt.clone() { v } else {
        let mut buf = String::new();
        match fs::File::open(&project_path).and_then(|mut f| f.read_to_string(&mut buf).map(|_| ())) {
            Ok(()) => {
                match serde_json::from_str::<serde_json::Value>(&buf)
                    .ok()
                    .and_then(|v| v.get("EngineAssociation").and_then(|x| x.as_str()).map(|s| s.to_string()))
                {
                    Some(assoc) => {
                        match crate::utils::resolve_engine_association_to_mm(&assoc) {
                            Some(mm) => mm,
                            None => {
                                return HttpResponse::NotFound().body("Could not resolve EngineAssociation from project to a version");
                            }
                        }
                    }
                    None => {
                        return HttpResponse::BadRequest().body("Project .uproject missing EngineAssociation and no version provided");
                    }
                }
            }
            Err(_) => {
                return HttpResponse::BadRequest().body("Failed to read project .uproject file to determine engine version");
            }
        }
    };
    println!("Requested engine version (resolved): {}", requested_version);

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

    let chosen = match utils::pick_engine_for_version(&engines, &requested_version) {
        Some(e) => e,
        None => {
            return HttpResponse::NotFound().body(format!("Requested version '{}' not found among discovered engines", requested_version));
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
    let request_body = body.into_inner();
    let job_id = request_body.job_id.clone();
    utils::emit_event(job_id.as_deref(), models::Phase::ImportStart, format!("Importing '{}'", request_body.asset_name), Some(0.0), None);

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
    if let (Some(namespace), Some(asset_id), Some(artifact_id)) = (request_body.namespace.clone(), request_body.asset_id.clone(), request_body.artifact_id.clone()) {
        // Forward jobId and ue parameters to the download handler
        let mut q: HashMap<String, String> = HashMap::new();
        if let Some(ref j) = job_id { q.insert("jobId".to_string(), j.clone()); }
        if let Some(ref ue) = request_body.ue { if !ue.trim().is_empty() { q.insert("ue".to_string(), ue.trim().to_string()); } }

        let path = web::Path::from((namespace.clone(), asset_id.clone(), artifact_id.clone()));
        let query: Query<HashMap<String, String>> = web::Query(q);
        match utils::download_asset_handler(path, query).await {
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
                    utils::epic_authenticate(&mut epic_services).await;
                }
                let friendly = utils::get_friendly_asset_name(&namespace, &asset_id, &artifact_id, &mut epic_services).await;
                let title_folder = utils::get_friendly_folder_name(friendly);
                let mut computed_asset_dir = downloads_base.join(title_folder.unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id)));
                if let Some(ref ue) = request_body.ue { if !ue.trim().is_empty() { computed_asset_dir = computed_asset_dir.join(ue.trim()); } }
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
    let safe_name = request_body.asset_name.trim();
    if safe_name.is_empty() {
        return HttpResponse::BadRequest().body("asset_name is required");
    }

    let mut asset_dir: PathBuf;
    if let (Some(namespace), Some(asset_id), Some(artifact_id)) = (request_body.namespace.clone(), request_body.asset_id.clone(), request_body.artifact_id.clone()) {
        // Recompute expected folder name like the downloader
        let mut epic_services = utils::create_epic_games_services();
        if !utils::try_cached_login(&mut epic_services).await {
            utils::epic_authenticate(&mut epic_services).await;
        }
        let friendly = utils::get_friendly_asset_name(&namespace, &asset_id, &artifact_id, &mut epic_services).await;
        let title_folder = utils::get_friendly_folder_name(friendly);
        let mut computed = downloads_base.join(title_folder.unwrap_or_else(|| format!("{}-{}-{}", namespace, asset_id, artifact_id)));
        if let Some(ref ue) = request_body.ue { if !ue.trim().is_empty() { computed = computed.join(ue.trim()); } }
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
    let project_dir = match utils::resolve_project_dir_from_param(&request_body.project) {
        Some(p) => p,
        None => return HttpResponse::BadRequest().body("Project could not be resolved to a valid Unreal project"),
    };
    let mut dest_content = project_dir.join("Content");
    if let Some(sub) = &request_body.target_subdir {
        let trimmed = sub.trim_matches(['/', '\\']);
        if !trimmed.is_empty() {
            dest_content = dest_content.join(trimmed);
        }
    }
    // Always create an asset-named subfolder inside the project's Content and copy into it.
    // Use a friendly, filesystem-safe folder name derived from the requested asset_name.
    let asset_folder_name = utils::get_friendly_folder_name(request_body.asset_name.clone()).unwrap_or_else(|| request_body.asset_name.clone());
    let dest_content = dest_content.join(asset_folder_name);

    let overwrite = request_body.overwrite.unwrap_or(false);
    let started = Instant::now();
    utils::emit_event(job_id.as_deref(), models::Phase::ImportCopying, format!("Copying files into {}", dest_content.display()), Some(0.0), None);
    match utils::copy_dir_recursive_with_progress(&src_content, &dest_content, overwrite, job_id.as_deref(), models::Phase::ImportCopying) {
        Ok((copied, skipped)) => {
            utils::emit_event(job_id.as_deref(), models::Phase::ImportComplete, format!("Imported '{}'", request_body.asset_name.trim()), Some(100.0), None);
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
            utils::emit_event(job_id.as_deref(), models::Phase::ImportError, format!("Failed to import: {}", e), None, None);
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
        let _ = obj.insert("EngineAssociation".to_string(), serde_json::Value::String(mm.clone()))
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

    utils::emit_event(job_id.as_deref(), models::Phase::CreateStart, format!("Creating project {}", req.project_name), None, None);

    // Handle Fab asset download if identifiers are provided
    if let Some(response) = utils::handle_fab_download(&req, &job_id).await {
        return response;
    }

    // Validate all inputs
    if let Err(response) = utils::validate_request(&req) {
        return response;
    }

    // Resolve engine path
    let engine_path = match utils::resolve_engine_path(&req) {
        Ok(path) => path,
        Err(response) => return response,
    };

    // Locate editor binary
    let editor_path = match utils::find_editor_binary(&engine_path) {
        Some(p) => p,
        None => return HttpResponse::BadRequest().body(
            "Unable to locate Unreal Editor binary under engine_path (tried UE5 'UnrealEditor' and UE4 'UE4Editor')"
        ),
    };

    // Resolve template .uproject file
    let template_path = match utils::resolve_template_path(&req, &job_id).await {
        Ok(path) => path,
        Err(response) => return response,
    };

    // Setup output directory
    let (out_dir, new_project_dir) = match utils::setup_output_directory(&req) {
        Ok(dirs) => dirs,
        Err(response) => return response,
    };

    let template_dir = template_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Handle dry run
    if req.dry_run.unwrap_or(false) {
        return utils::handle_dry_run(&req, &template_dir, &new_project_dir, &editor_path, &template_path);
    }

    // Copy project files
    let (copied_files, skipped_files) = match utils::copy_project_files(
        &template_dir,
        &new_project_dir,
        &req.project_name,
        &template_path,
        &job_id,
    ) {
        Ok(counts) => counts,
        Err(response) => return response,
    };

    utils::emit_event(
        job_id.as_deref(),
        models::Phase::CreateComplete,
        format!("Project created at {}", new_project_dir.to_string_lossy()),
        Some(100.0),
        None,
    );

    // Update .uproject metadata
    let target_uproject = utils::finalize_uproject(&new_project_dir, &req, &template_path);

    // Build and optionally execute open command
    let command_preview = utils::build_editor_command(&editor_path, &target_uproject, &req.project_type);
    println!("UnrealEditor: {}", editor_path.to_string_lossy());
    println!("Open Command: {}", command_preview);

    utils::execute_project_open(&req, copied_files, skipped_files, command_preview, &new_project_dir)
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
        utils::emit_event(Some(&jid), models::Phase::Cancelled, "Job cancelled", None, None);
        return HttpResponse::Ok().json(serde_json::json!({"ok": true, "message": "cancelled"}));
    }
    HttpResponse::BadRequest().body("missing jobId")
}



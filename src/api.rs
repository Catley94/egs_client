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

use actix_web::{get, post, HttpResponse, web, Responder};
use colored::Colorize;
use crate::utils;

use std::fs;
use std::io::Read;
use serde::Serialize;
use serde_json;
use std::path::{Path, PathBuf};
use std::time::Instant;

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

#[post("/get-fab-list")]
pub async fn get_fab_list_post() -> HttpResponse {
    // Allow clients using POST to hit the same logic
    let path = std::path::Path::new(FAB_CACHE_FILE);
    if path.exists() {
        if let Ok(mut f) = fs::File::open(path) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                println!("Using cached FAB list from {} (POST)", FAB_CACHE_FILE);
                return HttpResponse::Ok()
                    .content_type("application/json")
                    .body(buf);
            }
        }
    }
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
                    Ok(_) => {
                        println!("Download complete");
                        return HttpResponse::Ok().body("Download complete")
                    },
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

// ===== Unreal Projects Discovery =====

#[derive(Serialize)]
struct UnrealProjectInfo {
    name: String,
    path: String,
    uproject_file: String,
}

#[derive(Serialize)]
struct UnrealProjectsResponse {
    base_directory: String,
    projects: Vec<UnrealProjectInfo>,
}

fn default_unreal_projects_dir() -> PathBuf {
    // Default: $HOME/Documents/Unreal Projects
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
    let base_dir = query.get("base").map(|s| PathBuf::from(s)).unwrap_or_else(default_unreal_projects_dir);
    
    let mut results: Vec<UnrealProjectInfo> = Vec::new();

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
                                        let info = UnrealProjectInfo {
                                            name,
                                            path: path.to_string_lossy().to_string(),
                                            uproject_file: p.to_string_lossy().to_string(),
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

    let response = UnrealProjectsResponse {
        base_directory: base_dir.to_string_lossy().to_string(),
        projects: results,
    };

    HttpResponse::Ok().json(response)
}

// ===== Unreal Engines Discovery =====

#[derive(Serialize)]
struct UnrealEngineInfo {
    name: String,
    version: String,
    path: String,
    editor_path: Option<String>,
}

#[derive(Serialize)]
struct UnrealEnginesResponse {
    base_directory: String,
    engines: Vec<UnrealEngineInfo>,
}

fn default_unreal_engines_dir() -> PathBuf {
    // Default: $HOME/Unreal Engines
    if let Ok(home) = std::env::var("HOME") {
        let mut p = PathBuf::from(home);
        p.push("UnrealEngines");
        p
    } else {
        PathBuf::from(".")
    }
}

fn read_build_version(engine_dir: &Path) -> Option<String> {
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

fn find_editor_binary(engine_dir: &Path) -> Option<PathBuf> {
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

fn parse_version_from_name(name: &str) -> Option<String> {
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
    let base_dir = query.get("base").map(|s| PathBuf::from(s)).unwrap_or_else(default_unreal_engines_dir);

    let mut engines: Vec<UnrealEngineInfo> = Vec::new();
    if base_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Heuristic: consider any directory that has Engine/Binaries
                    if path.join("Engine").join("Binaries").is_dir() {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        let version = read_build_version(&path)
                            .or_else(|| parse_version_from_name(&name))
                            .unwrap_or_else(|| "unknown".to_string());
                        let editor_path = find_editor_binary(&path).map(|p| p.to_string_lossy().to_string());
                        engines.push(UnrealEngineInfo {
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

    let resp = UnrealEnginesResponse {
        base_directory: base_dir.to_string_lossy().to_string(),
        engines,
    };

    HttpResponse::Ok().json(resp)
}

#[derive(Serialize)]
struct OpenProjectResponse {
    launched: bool,
    engine_name: Option<String>,
    engine_version: Option<String>,
    editor_path: Option<String>,
    project: String,
    message: String,
}

fn resolve_project_path(project_param: &str) -> Option<PathBuf> {
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

fn pick_engine_for_version<'a>(engines: &'a [UnrealEngineInfo], requested: &str) -> Option<&'a UnrealEngineInfo> {
    // Try exact version match first
    if let Some(e) = engines.iter().find(|e| e.version == requested) { return Some(e); }
    // Try prefix match (e.g., request 5.3 and engine 5.3.2)
    if let Some(e) = engines.iter().find(|e| e.version.starts_with(requested)) { return Some(e); }
    // Try name contains requested (e.g., UE_5.3)
    engines.iter().find(|e| e.name.contains(requested))
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
    let engine_base = query.get("engine_base").map(|s| PathBuf::from(s)).unwrap_or_else(default_unreal_engines_dir);
    let projects_base = query
        .get("projects_base")
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(default_unreal_projects_dir);

    // First try to resolve as path/dir; if that fails, treat `raw_project` as a project name
    let project_path = match resolve_project_path(&raw_project) {
        Some(p) => Some(p),
        None => {
            // Interpret as a name: search projects_base/<name> for a .uproject file
            let candidate_dir = projects_base.join(&raw_project);
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
        Some(p) => p,
        None => {
            return HttpResponse::BadRequest().body("Project not found by path or name, or no .uproject in directory");
        }
    };

    // Discover engines
    let mut engines: Vec<UnrealEngineInfo> = Vec::new();
    if engine_base.is_dir() {
        if let Ok(entries) = fs::read_dir(&engine_base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.join("Engine").join("Binaries").is_dir() {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        let version = read_build_version(&path)
                            .or_else(|| parse_version_from_name(&name))
                            .unwrap_or_else(|| "unknown".to_string());
                        let editor_path = find_editor_binary(&path).map(|p| p.to_string_lossy().to_string());
                        engines.push(UnrealEngineInfo { name, version, path: path.to_string_lossy().to_string(), editor_path });
                    }
                }
            }
        }
    }

    if engines.is_empty() {
        return HttpResponse::NotFound().body("No Unreal Engine installations found in engine_base");
    }

    let chosen = match pick_engine_for_version(&engines, &version_param) {
        Some(e) => e,
        None => {
            return HttpResponse::NotFound().body("Requested version not found among discovered engines");
        }
    };

    let editor_path = match &chosen.editor_path {
        Some(p) => PathBuf::from(p),
        None => return HttpResponse::NotFound().body("Engine found but Editor binary not located"),
    };

    // Spawn the editor without waiting for it to exit
    let spawn_res = std::process::Command::new(&editor_path)
        .arg(&project_path)
        .spawn();

    match spawn_res {
        Ok(_child) => {
            let resp = OpenProjectResponse {
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
            let resp = OpenProjectResponse {
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



/// Request payload for importing a downloaded asset into a UE project.
#[derive(serde::Deserialize)]
pub struct ImportAssetRequest {
    /// Asset folder name as stored under downloads/ (e.g., "Industry Props Pack 6").
    pub asset_name: String,
    /// Project identifier: name, project directory, or path to .uproject
    pub project: String,
    /// Optional subfolder inside Project/Content to copy into (e.g., "Imported/Industry").
    pub target_subdir: Option<String>,
    /// When true, overwrite existing files. When false, skip existing files.
    pub overwrite: Option<bool>,
}

#[derive(Serialize)]
struct ImportAssetResponse {
    ok: bool,
    message: String,
    files_copied: usize,
    files_skipped: usize,
    source: String,
    destination: String,
    elapsed_ms: u128,
}

fn resolve_project_dir_from_param(param: &str) -> Option<PathBuf> {
    // Reuse the existing resolver; it returns a .uproject path when found
    if let Some(p) = resolve_project_path(param) {
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

fn copy_dir_recursive(src: &Path, dst: &Path, overwrite: bool) -> std::io::Result<(usize, usize)> {
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
pub async fn import_asset(body: web::Json<ImportAssetRequest>) -> impl Responder {
    let req = body.into_inner();
    // Resolve source: downloads/<asset_name>/data/Content
    let safe_name = req.asset_name.trim();
    if safe_name.is_empty() {
        return HttpResponse::BadRequest().body("asset_name is required");
    }
    let src_content = Path::new("downloads").join(safe_name).join("data").join("Content");
    if !src_content.is_dir() {
        return HttpResponse::NotFound().body(format!("Source Content folder not found: {}", src_content.display()));
    }

    // Resolve project directory and destination Content
    let project_dir = match resolve_project_dir_from_param(&req.project) {
        Some(p) => p,
        None => return HttpResponse::BadRequest().body("Project could not be resolved to a valid Unreal project"),
    };
    let mut dest_content = project_dir.join("Content");
    if let Some(sub) = &req.target_subdir {
        let trimmed = sub.trim_matches(['/','\\']);
        if !trimmed.is_empty() {
            dest_content = dest_content.join(trimmed);
        }
    }

    let overwrite = req.overwrite.unwrap_or(false);
    let started = Instant::now();
    match copy_dir_recursive(&src_content, &dest_content, overwrite) {
        Ok((copied, skipped)) => {
            let resp = ImportAssetResponse {
                ok: true,
                message: format!("Imported '{}' into project at {}", safe_name, project_dir.display()),
                files_copied: copied,
                files_skipped: skipped,
                source: src_content.to_string_lossy().to_string(),
                destination: dest_content.to_string_lossy().to_string(),
                elapsed_ms: started.elapsed().as_millis(),
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => {
            let resp = ImportAssetResponse {
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


// #[get("/health")]
// pub async fn health() -> HttpResponse {
//     HttpResponse::Ok().body("OK")
// }

// #[get("/")]
// pub async fn root() -> HttpResponse {
//     HttpResponse::Ok().body(
//         "egs_client is running. Try /health, /get-fab-list, or /refresh-fab-list."
//     )
// }

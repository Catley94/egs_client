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
use serde::{Serialize, Deserialize};
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

/// POST alias for clients that send POST requests to the same endpoint.
///
/// Route:
/// - POST /get-fab-list
///
/// Behavior and Returns: same as GET /get-fab-list.
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
    // Allow override via env var EGS_UNREAL_PROJECTS_DIR
    if let Ok(val) = std::env::var("EGS_UNREAL_PROJECTS_DIR") {
        if !val.trim().is_empty() {
            return PathBuf::from(val);
        }
    }
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
    // Allow override via env var EGS_UNREAL_ENGINES_DIR
    if let Ok(val) = std::env::var("EGS_UNREAL_ENGINES_DIR") {
        if !val.trim().is_empty() {
            return PathBuf::from(val);
        }
    }
    // Default: $HOME/UnrealEngines
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

#[derive(Serialize)]
struct OpenEngineResponse {
    launched: bool,
    engine_name: Option<String>,
    engine_version: Option<String>,
    editor_path: Option<String>,
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
    println!("Project Base: {}", projects_base.to_string_lossy());
    println!("Raw Project: {}", raw_project);
    println!("Engine Base: {}", engine_base.to_string_lossy());
    println!("Version: {}", version_param);

    // First try to resolve as path/dir; if that fails, treat `raw_project` as a project name
    let project_path = match resolve_project_path(&raw_project) {
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
    println!("Using editor: {}", editor_path.to_string_lossy());

    // Spawn the editor without waiting for it to exit
    let spawn_res = std::process::Command::new(&editor_path)
        .arg(&project_path)
        .spawn();
    println!("Spawn Result: {:?}", spawn_res);

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

/// Ensure an asset with the given library title is available under downloads/.
/// If not present, attempts to authenticate, locate the asset in the Fab library,
/// pick one of its project_versions (latest if possible), and download it.
/// Returns the asset folder path under downloads/ on success.
async fn ensure_asset_downloaded_by_name(title: &str) -> Result<PathBuf, String> {
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
    if asset_dir.exists() { return Ok(asset_dir); }

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
                match utils::download_asset(&dm, url.as_str(), &out_root).await {
                    Ok(_) => { return Ok(out_root); },
                    Err(e) => { eprintln!("Download failed from {}: {:?}", url, e); continue; }
                }
            }
        }
    }
    Err("Unable to download asset from any distribution point".to_string())
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
    // Resolve source: downloads/<asset_name>/data/Content (download first if missing)
    let safe_name = req.asset_name.trim();
    if safe_name.is_empty() {
        return HttpResponse::BadRequest().body("asset_name is required");
    }
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
    // Try exact and case-insensitive match
    let mut asset_dir = downloads_base.join(safe_name);
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
    if !asset_dir.exists() {
        // Attempt to download the asset by name
        match ensure_asset_downloaded_by_name(safe_name).await {
            Ok(path) => { asset_dir = path; },
            Err(err) => { return HttpResponse::NotFound().body(format!("{}", err)); }
        }
    }
    let src_content = asset_dir.join("data").join("Content");
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

#[derive(Serialize, Deserialize)]
struct CreateUnrealProjectRequest {
    engine_path: Option<String>,
    /// Path to a template/sample .uproject OR a directory containing one. If omitted, provide asset_name.
    template_project: Option<String>,
    /// Convenience: name of a downloaded asset under downloads/ (e.g., "Stack O Bot").
    /// When provided and template_project is empty, the server will search downloads/<asset_name>/ recursively for a .uproject.
    asset_name: Option<String>,
    output_dir: String,
    project_name: String,
    project_type: Option<String>, // "bp" or "cpp"
    /// When true, launch Unreal Editor to open the created project after copying. Defaults to false.
    open_after_create: Option<bool>,
    dry_run: Option<bool>,
}

#[derive(Serialize)]
struct CreateUnrealProjectResponse {
    ok: bool,
    message: String,
    command: String,
    project_path: Option<String>,
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
pub async fn create_unreal_project(body: web::Json<CreateUnrealProjectRequest>) -> impl Responder {
    use serde::Deserialize;
    let req = body.into_inner();

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

    // Resolve engine path: try provided, else discover from default engines dir and pick latest
    let engine_path = if let Some(p) = req.engine_path.clone() { PathBuf::from(p) } else {
        let base = default_unreal_engines_dir();
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
        // If still missing, attempt to download by name first
        if !asset_dir.exists() {
            match ensure_asset_downloaded_by_name(name).await {
                Ok(p) => { asset_dir = p; },
                Err(err) => { eprintln!("{}", err); }
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
                }
            }
        } else if entry.file_type().is_symlink() {
            // Skip symlinks to avoid unexpected behavior
            skipped_files += 1;
        }
    }

    println!("[copy-finish] Copied {} files ({} skipped) to {}",
        copied_files, skipped_files, new_project_dir.to_string_lossy());

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
    if let Ok(mut json_text) = fs::read_to_string(&target_uproject) {
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
        let resp = CreateUnrealProjectResponse { ok: true, message: format!("Dry run: would copy {} files (skipped {}), then open project{}", copied_files, skipped_files, if req.open_after_create.unwrap_or(false) { " (open_after_create=true)" } else { "" }), command: actions.join(" | "), project_path: Some(new_project_dir.to_string_lossy().to_string()) };
        return HttpResponse::Ok().json(resp);
    }

    // Decide whether to open after create (default false)
    let open_after = req.open_after_create.unwrap_or(false);
    if open_after {
        match cmd.spawn() {
            Ok(_child) => {
                let resp = CreateUnrealProjectResponse {
                    ok: true,
                    message: format!("Project created ({} files, {} skipped). Unreal Editor is launching...", copied_files, skipped_files),
                    command: command_preview,
                    project_path: Some(new_project_dir.to_string_lossy().to_string()),
                };
                return HttpResponse::Ok().json(resp);
            }
            Err(e) => {
                let resp = CreateUnrealProjectResponse {
                    ok: true, // project created successfully; opening is optional
                    message: format!("Project created ({} files, {} skipped). Failed to launch UnrealEditor: {}", copied_files, skipped_files, e),
                    command: command_preview,
                    project_path: Some(new_project_dir.to_string_lossy().to_string()),
                };
                return HttpResponse::Ok().json(resp);
            }
        }
    } else {
        let resp = CreateUnrealProjectResponse {
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
        .unwrap_or_else(default_unreal_engines_dir);

    println!("Engine Base: {}", engine_base.to_string_lossy());
    println!("Version: {}", version_param);

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

    println!("Using editor: {}", editor_path.to_string_lossy());

    // Spawn the editor without waiting for it to exit (no project argument)
    let spawn_res = std::process::Command::new(&editor_path).spawn();
    println!("Spawn Result: {:?}", spawn_res);

    match spawn_res {
        Ok(_child) => {
            let resp = OpenEngineResponse {
                launched: true,
                engine_name: Some(chosen.name.clone()),
                engine_version: Some(chosen.version.clone()),
                editor_path: Some(editor_path.to_string_lossy().to_string()),
                message: "Launched Unreal Editor".to_string(),
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => {
            let resp = OpenEngineResponse {
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

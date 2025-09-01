use std::time::Duration;
use actix_web::{get, HttpResponse, web};
use colored::Colorize;
use egs_api::api::error::EpicAPIError;
use tokio::time::sleep;
use crate::utils;
// Rust-like outline
use std::path::Path;

use egs_api::api::types::download_manifest::DownloadManifest;

use std::fs;
use std::io::Read;
use serde_json;

const FAB_CACHE_DIR: &str = "cache";
const FAB_CACHE_FILE: &str = "cache/fab_list.json";

#[get("/get-fab-list")]
pub async fn get_fab_list() -> HttpResponse {
    // Try cached file first
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

#[get("/refresh-fab-list")]
pub async fn refresh_fab_list() -> HttpResponse {
    // Respond with the list of Fab Assets and cache it

    handle_refresh_fab_list().await
}

// Refreshes manifests only (no downloads) and returns a summary list
pub async fn handle_refresh_fab_list() -> HttpResponse {
    // Try to use cached refresh token first (no browser, no copy-paste)
    let mut epic_games_services = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic_games_services).await {

        let auth_code = utils::get_auth_code();

        // 3. Authenticate with Epic's Servers using the code
        if epic_games_services.auth_code(None, Some(auth_code)).await {
            println!("Logged in with provided auth code");
        }
        // Complete login; Epic SDK should fill user_details with tokens
        let _ = epic_games_services.login().await;

        // Persist tokens for next runs
        let ud = epic_games_services.user_details();
        if let Err(e) = utils::save_user_details(&ud) {
            eprintln!("Warning: failed to save tokens: {}", e);
        }
    } else {
        println!("Logged in using cached credentials");
    }

    // 4. Get account details
    let details = utils::get_account_details(&mut epic_games_services).await;
    // println!("Account details: {:?}", details);

    // 5. Get account details
    let info = utils::get_account_info(&mut epic_games_services).await;
    // println!("Account info: {:?}", info);

    // 6. Get library items
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
                    // Save to cache file
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
                // Ensure SourceURL present for downloader
                use std::collections::HashMap;
                if let Some(ref mut fields) = dm.custom_fields {
                    fields.insert("SourceURL".to_string(), url.clone());
                } else {
                    let mut map = HashMap::new();
                    map.insert("SourceURL".to_string(), url.clone());
                    dm.custom_fields = Some(map);
                }

                // Resolve a human-friendly title for folder name
                let mut title_folder: Option<String> = None;
                // Try to use the library list to find the matching asset by IDs
                if let Some(details) = utils::get_account_details(&mut epic).await {
                    if let Some(lib) = utils::get_fab_library_items(&mut epic, details).await {
                        if let Some(asset) = lib.results.iter().find(|a| a.asset_namespace == namespace && a.asset_id == asset_id) {
                            // Optionally verify artifact exists in this asset's versions
                            if asset.project_versions.iter().any(|v| v.artifact_id == artifact_id) {
                                let mut t = asset.title.clone();
                                // sanitize
                                let illegal: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
                                t = t.replace(&illegal[..], "_");
                                // trim spaces and dots
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


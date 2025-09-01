use std::time::Duration;
use actix_web::{get, HttpResponse};
use colored::Colorize;
use egs_api::api::error::EpicAPIError;
use tokio::time::sleep;
use crate::utils;
// Rust-like outline
use std::path::Path;

use egs_api::api::types::download_manifest::DownloadManifest;



#[get("/get-fab-list")]
pub async fn get_fab_list() -> HttpResponse {
    // Respond with the list of Fab Assets
    // If cached, return cached list
    // If not cached, refresh list and cache it - refresh_fab_list()

    // handle_refresh_fab_list().await

    HttpResponse::Ok().finish()
}

#[get("/refresh-fab-list")]
pub async fn refresh_fab_list() -> HttpResponse {
    // Respond with the list of Fab Assets and cache it

    handle_refresh_fab_list().await
}

pub async fn handle_refresh_fab_list() -> HttpResponse {
    // Try to use cached refresh token first (no browser, no copy-paste)
    let mut epic_games_services = utils::create_epic_games_services();
    if !utils::try_cached_login(&mut epic_games_services).await {
        // Fallback to manual auth code flow
        // 1. Open the Web Browser for the user to get the authentication code
        // 2. Get auth code from user via CLI
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
    println!("Account details: {:?}", details);

    // 5. Get account details
    let info = utils::get_account_info(&mut epic_games_services).await;
    println!("Account info: {:?}", info);

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

                    // Process all assets instead of just the first one
                    for (asset_idx, asset) in retrieved_assets.results.iter().enumerate() {
                        let asset_num = asset_idx + 1;
                        let total_assets = retrieved_assets.results.len();
                        
                        println!("{}", format!("                        PROCESSING_ASSET_{}_OF_{}                        ", asset_num, total_assets).black().on_bright_cyan().bold());
                        
                        for version in asset.project_versions.iter() {
                            loop {
                                let manifest = epic_games_services.fab_asset_manifest(
                                    &version.artifact_id,
                                    &asset.asset_namespace,
                                    &asset.asset_id,
                                    None,
                                ).await;
                                match manifest {
                                    Ok(manifest) => {
                                        println!("{}", format!("                        ASSET_{}_OF_{}: {}                        ", asset_num, total_assets, asset.title).black().on_bright_cyan().bold());
                                        println!("OK Manifest for {} - {} - {}", asset.title, version.artifact_id, asset.source);
                                        println!("______________________________________ASSET_{}________________________________________________________________", asset_num);
                                        println!("Full Asset: {:?}", asset);
                                        println!("_________________________________________MANIFEST_{}___________________________________________________________", asset_num);
                                        println!("Full Manifest: {:?}", manifest);
                                        println!("_________________________________________________________________________________________________________________");

                                        println!("{}", format!("Downloading asset {} of {}: {}...", asset_num, total_assets, asset.title).green().bold());

                                        // Iterate through distribution points to find a working download URL
                                        for man in manifest.iter() {
                                            let mut downloaded = false;
                                            for url in man.distribution_point_base_urls.iter() {
                                                println!("Trying to get download manifest from {}", url);
                                                let download_manifest = epic_games_services.fab_download_manifest(man.clone(), url).await;
                                                match download_manifest {
                                                    Ok(mut dm) => {

                                                        // Ensure the manifest knows the source URL so egs_api can generate chunk links
                                                        // DownloadManifest.files() uses custom_fields["SourceURL"] or ["BaseUrl"]
                                                        {
                                                            use std::collections::HashMap;
                                                            if let Some(ref mut fields) = dm.custom_fields {
                                                                fields.insert("SourceURL".to_string(), url.clone());
                                                            } else {
                                                                let mut map = HashMap::new();
                                                                map.insert("SourceURL".to_string(), url.clone());
                                                                dm.custom_fields = Some(map);
                                                            }
                                                        }

                                                        println!("{}", "Got download manifest successfully!".green());
                                                        println!("Expected Hash: {}", man.manifest_hash);
                                                        println!("Download Hash: {}", dm.custom_field("DownloadedManifestHash").unwrap_or_default());

                                                        // Build an output folder under the project root for this asset
                                                        let out_root = std::path::Path::new("downloads")
                                                            .join(asset.title.replace(&['/', '\\', ':', '*', '?', '"', '<', '>','|'][..], "_"));

                                                        // Call your downloader using the working distribution point URL and the dm
                                                        match utils::download_asset(&dm, url.as_str(), &out_root).await {
                                                            Ok(_) => {
                                                                println!("✅ Finished downloading asset {} of {}: {} to {}", asset_num, total_assets, asset.title, out_root.display());
                                                            }
                                                            Err(e) => {
                                                                println!("❌ Download failed for asset {} of {}: {} - {:?}", asset_num, total_assets, asset.title, e);
                                                            }
                                                        }

                                                        downloaded = true;
                                                        break;
                                                    }
                                                    Err(e) => {
                                                        match e {
                                                            EpicAPIError::FabTimeout => {
                                                                sleep(Duration::from_millis(1000)).await;
                                                                continue;
                                                            }
                                                            _ => {}
                                                        }
                                                        println!("NO Manifest for {} - {}", asset.title, version.artifact_id);
                                                        break;
                                                    }
                                                }
                                            }
                                            sleep(Duration::from_millis(1000)).await;
                                        }
                                        break; // Exit the loop once we've processed this manifest
                                    }
                                    Err(e) => {
                                        match e {
                                            EpicAPIError::FabTimeout => {
                                                sleep(Duration::from_millis(1000)).await;
                                                continue;
                                            }
                                            _ => {
                                                println!("NO Manifest for {} - {}", asset.title, version.artifact_id);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    println!("{}", "🎉 Finished processing all assets!".green().bold());
                    HttpResponse::Ok().finish()
                }
            }
        }
    }
}






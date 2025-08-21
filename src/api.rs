use std::time::Duration;
use actix_web::{main, get, post, web, App, HttpResponse, HttpServer, Responder};
use colored::Colorize;
use egs_api::api::error::EpicAPIError;
use egs_api::api::types::account::{AccountData, AccountInfo};
use egs_api::api::types::fab_library::FabLibrary;
use egs_api::EpicGames;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use crate::utils;

#[get("/get-fab-list")]
async fn get_fab_list() -> HttpResponse {
    // Respond with the list of Fab Assets
    // If cached, return cached list
    // If not cached, refresh list and cache it - refresh_fab_list()

    // handle_refresh_fab_list().await

    HttpResponse::Ok().finish()
}

#[get("/refresh-fab-list")]
async fn refresh_fab_list() -> HttpResponse {
    // Respond with the list of Fab Assets and cache it

    handle_refresh_fab_list().await

    // HttpResponse::Ok().finish()
}

async fn handle_refresh_fab_list() -> HttpResponse {
    // 1. Open the Web Browser for the user to get the authentication code
    // 2. Get auth code from user via CLI
    // TODO: Can I automate this process?
    let auth_code = utils::get_auth_code();

    // 3. Create EpicGames object and login to Epic's Servers
    let mut epic_games_services = utils::create_epic_games_services();
    if epic_games_services.auth_code(None, Some(auth_code)).await {
        println!("Logged in");
    }
    epic_games_services.login().await;

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
        }
        Some(info) => {
            let assets = utils::get_fab_library_items(&mut epic_games_services, info).await;
            match assets {
                None => {
                    println!("No assets found");
                }
                Some(retrieved_assets) => {
                    println!("Library items length: {:?}", retrieved_assets.results.len());

                    for (index, asset) in retrieved_assets.results.iter().enumerate() {

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
                                        println!("{}", format!("                                       ITEM_{}                                                     ", index).black().on_bright_cyan().bold());
                                        println!("OK Manifest for {} - {} - {}", asset.title, version.artifact_id, asset.source);
                                        println!("______________________________________FULL_ASSET_________________________________________________________________");
                                        println!("Full Manifest for {:?}", asset);
                                        println!("_________________________________________FULL_MANIFEST___________________________________________________________");
                                        println!("Full Manifest for {:?}", manifest);
                                        println!("_________________________________________________________________________________________________________________");

                                        /*
                                            Example manifest:
                                            Full Manifest for FabAsset
                                            {
                                                asset_id: "28b7df0e7f5e4202be89a20d362860c3",
                                                asset_namespace: "89efe5924d3d467c839449ab6ab52e7f",
                                                categories: [
                                                    Category { id: "ad152ac0-0e9c-4233-9a5c-f29050798a38", name: Some("Containers") }
                                                ],
                                                custom_attributes: [
                                                    {"ListingIdentifier": "b5603e44-e1b0-4346-9c3d-04887aa9f87d"}
                                                ],
                                                description: "Industry Props Pack 6",
                                                distribution_method: "ASSET_PACK",
                                                images: [
                                                    Image {
                                                        height: "349",
                                                        md5: None,
                                                        type_field: "Featured",
                                                        uploaded_date: "2024-12-06T09:49:02.319407Z",
                                                        url: "https://media.fab.com/image_previews/gallery_images/f0b6dd69-3768-4763-8f65-b33fb1f4c3c3/bb63531b-e5b9-4454-99ea-3fdb17892cb0.jpg",
                                                        width: "640"
                                                    }
                                                ], legacy_item_id: Some("f4a3f3ff297f43ac92e0dda0b5bc351e"),
                                                project_versions: [
                                                    ProjectVersion {
                                                        artifact_id: "Industryf4a3f3ff297fV1",
                                                        build_versions: [
                                                            BuildVersion {
                                                                build_version: "5.6.0-40032047+++UE5+Dev-Marketplace-Windows",
                                                                platform: "Windows"
                                                            }
                                                        ],
                                                        engine_versions: [
                                                            "UE_4.18",
                                                            "UE_4.19",
                                                            "UE_4.20",
                                                            "UE_4.21",
                                                            "UE_4.22",
                                                            "UE_4.23",
                                                            "UE_4.24",
                                                            "UE_4.25",
                                                            "UE_4.26",
                                                            "UE_4.27",
                                                            "UE_5.0",
                                                            "UE_5.1",
                                                            "UE_5.2",
                                                            "UE_5.3",
                                                            "UE_5.4",
                                                            "UE_5.5",
                                                            "UE_5.6"
                                                        ],
                                                        target_platforms: ["Windows"]
                                                    }
                                                ],
                                                source: "fab",
                                                title: "Industry Props Pack 6",
                                                url: "https://www.fab.com/listings/b5603e44-e1b0-4346-9c3d-04887aa9f87d"
                                            }
                                        */
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
                    }
                }
            }
        }
    }

    HttpResponse::Ok().finish()
}






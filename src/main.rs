use egs_api::EpicGames;
use std::io::{self};
use std::time::Duration;
use egs_api::api::error::EpicAPIError;
use tokio::time::sleep;
use colored::*;
// Create/open a text file for writing
use std::fs::File;

const EPIC_LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

#[tokio::main]
async fn main() {
    env_logger::init();

    // 1.  the Web Browser for the user to get the authentication code
    // TODO: Can I automate this process?
    if webbrowser::open(EPIC_LOGIN_URL).is_err() {
        println!("Please go to {}", EPIC_LOGIN_URL)
    }
    println!("Please enter the 'authorizationCode' value from the JSON response");

    // 2. Get auth code from user via CLI
    // TODO: Can I automate this process?
    let auth_code = get_auth_code();

    // 3. Create EpicGames object and login to Epic's Servers
    let mut epic_games_services = EpicGames::new();
    if epic_games_services.auth_code(None, Some(auth_code)).await {
        println!("Logged in");
    }
    epic_games_services.login().await;

    // 4. Get account details
    let details = epic_games_services.account_details().await;
    println!("Account details: {:?}", details);

    // 5. Get account details
    let info = epic_games_services
        .account_ids_details(vec![epic_games_services.user_details().account_id.unwrap_or_default()])
        .await;
    println!("Account info: {:?}", info);

    // let friends = egs.account_friends(true).await;
    // println!("Friends: {:?}", friends);

    // 6. Get library items
    match details {
        None => {
            println!("No details found");
        }
        Some(info) => {
            let assets = epic_games_services.fab_library_items(info.id).await;
            match assets {
                None => {
                    println!("No assets found");
                }
                Some(ass) => {
                    println!("Library items: {:?}", ass.results.len());



                    let mut file = File::create("assets_reference.txt").expect("Could not create file");

                    for (index, asset) in ass.results.iter().enumerate() {


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

                                        // Write asset info to file
                                        // writeln!(file, "=== Asset {} ===", index + 1).expect("Could not write to file");
                                        // writeln!(file, "Title: {}", asset.title).expect("Could not write to file");
                                        // writeln!(file, "Asset ID: {}", asset.asset_id).expect("Could not write to file");
                                        // writeln!(file, "Asset Namespace: {}", asset.asset_namespace).expect("Could not write to file");
                                        // writeln!(file, "Description: {}", asset.description).expect("Could not write to file");
                                        // writeln!(file, "Full Asset Data: {:?}", asset).expect("Could not write to file");
                                        // writeln!(file, "Full Manifest Data: {:?}", manifest).expect("Could not write to file");
                                        // writeln!(file, "").expect("Could not write to file"); // Empty line for separation

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

    // let manifest = egs
    //     .fab_asset_manifest(
    //         "KiteDemo473",
    //         "89efe5924d3d467c839449ab6ab52e7f",
    //         "28166226c38a4ff3aa28bbe87dcbbe5b",
    //         None,
    //     )
    //     .await;
    // println!("Kite Demo Manifest: {:#?}", manifest);
    // if let Ok(manif) = manifest {
    //     for man in manif.iter() {
    //         for url in man.distribution_point_base_urls.iter() {
    //             println!("Trying to get download manifest from {}", url);
    //             let dm = egs.fab_download_manifest(man.clone(), url).await;
    //             match dm {
    //                 Ok(d) => {
    //                     println!("Got download manifest from {}", url);
    //                     println!("Expected Hash: {}", man.manifest_hash);
    //                     println!("Download Hash: {}", d.custom_field("DownloadedManifestHash").unwrap_or_default());
    //                 }
    //                 Err(_) => {}
    //             }
    //         }
    //     }
    // };
    //
    // let code = egs.game_token().await;
    // if let Some(c) = code {
    //     let authorized_url = format!("https://www.epicgames.com/id/exchange?exchangeCode={}&redirectUrl=https%3A%2F%2Fwww.unrealengine.com%2Fdashboard%3Flang%3Den", c.code);
    //     if webbrowser::open(&authorized_url).is_err() {
    //         println!("Please go to {}", authorized_url)
    //     }
    // }
    //
    // let assets = egs.list_assets(None, None).await;
    // let mut ueasset_map: HashMap<String, HashMap<String, EpicAsset>> = HashMap::new();
    // let mut non_ueasset_map: HashMap<String, HashMap<String, EpicAsset>> = HashMap::new();
    // for asset in assets {
    //     if asset.namespace == "ue" {
    //         if !ueasset_map.contains_key(&asset.catalog_item_id.clone()) {
    //             ueasset_map.insert(asset.catalog_item_id.clone(), HashMap::new());
    //         };
    //         match ueasset_map.get_mut(&asset.catalog_item_id.clone()) {
    //             None => {}
    //             Some(old) => {
    //                 old.insert(asset.app_name.clone(), asset.clone());
    //             }
    //         };
    //     } else {
    //         if !non_ueasset_map.contains_key(&asset.catalog_item_id.clone()) {
    //             non_ueasset_map.insert(asset.catalog_item_id.clone(), HashMap::new());
    //         };
    //         match non_ueasset_map.get_mut(&asset.catalog_item_id.clone()) {
    //             None => {}
    //             Some(old) => {
    //                 old.insert(asset.app_name.clone(), asset.clone());
    //             }
    //         };
    //     }
    // }
    //
    // println!("Got {} assets", ueasset_map.len() + non_ueasset_map.len());
    // println!("From that {} unreal assets", ueasset_map.len());
    // println!("From that {} non unreal assets", non_ueasset_map.len());
    //
    // // for (key, value) in ueasset_map.iter() {
    // //     println!("{}: {}", key, value.len());
    // // }
    //
    // println!("Getting the asset metadata");
    // // Get the last item in the asset list
    // let test_asset = ueasset_map
    //     .values()
    //     .last()
    //     .unwrap()
    //     .values()
    //     .last()
    //     .unwrap()
    //     .to_owned();
    // egs.asset_manifest(
    //     None,
    //     None,
    //     Some(test_asset.namespace.clone()),
    //     Some(test_asset.catalog_item_id.clone()),
    //     Some(test_asset.app_name.clone()),
    // )
    // .await;
    // println!("{:#?}", test_asset.clone());
    // println!("Getting the asset info");
    // let mut categories: HashSet<String> = HashSet::new();
    // for (_guid, asset) in non_ueasset_map.clone() {
    //     match egs
    //         .asset_info(asset.values().last().unwrap().to_owned())
    //         .await
    //     {
    //         None => {}
    //         Some(info) => {
    //             for category in info.categories.unwrap() {
    //                 categories.insert(category.path);
    //             }
    //         }
    //     };
    // }
    // let mut cat: Vec<String> = categories.into_iter().collect();
    // cat.sort();
    // for category in cat {
    //     println!("{}", category);
    // }
    // let _asset_info = egs.asset_info(test_asset.clone()).await;
    // println!("Getting ownership token");
    // egs.ownership_token(test_asset.clone()).await;
    // println!("Getting the game token");
    // egs.game_token().await;
    // println!("Getting the entitlements");
    // egs.user_entitlements().await;
    // println!("Getting the library items");
    // egs.library_items(true).await;
    // println!("Getting Asset manifest");
    // let manifest = egs
    //     .asset_manifest(
    //         None,
    //         None,
    //         Some(test_asset.namespace.clone()),
    //         Some(test_asset.catalog_item_id.clone()),
    //         Some(test_asset.app_name.clone()),
    //     )
    //     .await;
    // println!("{:?}", manifest);
    //
    // let download_manifest = egs.asset_download_manifests(manifest.unwrap()).await;
}

fn get_auth_code() -> (String) {
    let mut auth_code = String::new();
    let stdin = io::stdin(); // We get `Stdin` here.
    stdin.read_line(&mut auth_code).unwrap();
    auth_code = auth_code.trim().to_string();
    auth_code = auth_code.replace(|c: char| c == '"', "");
    println!("Using Auth Code: {}", auth_code);
    auth_code
}

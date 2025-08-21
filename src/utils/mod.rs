use std::io;
use egs_api::api::types::account::{AccountData, AccountInfo};
use egs_api::api::types::fab_library::FabLibrary;
use egs_api::EpicGames;

const EPIC_LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

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

pub fn create_epic_games_services() -> EpicGames {
    EpicGames::new()
}

pub async fn get_account_details(epic_games_services: &mut EpicGames) -> Option<AccountData> {
    // TODO What's the difference between this and get_account_info?
    epic_games_services.account_details().await
}

pub async fn get_account_info(mut epic_games_services: &mut EpicGames) -> Option<Vec<AccountInfo>> {
    // TODO What's the difference between this and get_account_details?
    epic_games_services
        .account_ids_details(vec![epic_games_services.user_details().account_id.unwrap_or_default()])
        .await
}

pub async fn get_fab_library_items(epic_games_services: &mut EpicGames, info: AccountData) -> Option<FabLibrary> {
    epic_games_services.fab_library_items(info.id).await
}



// Old Code that may be useful

// let friends = egs.account_friends(true).await;
// println!("Friends: {:?}", friends);



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
use std::time::Duration;
use actix_web::{get, HttpResponse};
use colored::Colorize;
use egs_api::api::error::EpicAPIError;
use tokio::time::sleep;
use crate::utils;
// Rust-like outline
use std::path::Path;

use egs_api::api::types::download_manifest::DownloadManifest;

async fn download_asset(dm: &DownloadManifest, _base_url: &str, out_root: &Path) -> Result<(), anyhow::Error> {
    use egs_api::api::types::chunk::Chunk;
    use sha1::{Digest, Sha1};
    use std::io::{self, Write};

    // Create base output dirs
    std::fs::create_dir_all(out_root)?;
    let temp_dir = out_root.parent().map(|p| p.join("temp")).unwrap_or_else(|| out_root.join("temp"));
    std::fs::create_dir_all(&temp_dir)?;

    let client = reqwest::Client::new();

    let files: Vec<_> = dm.files().into_iter().collect();
    // println!("Files: {:?}", files);
    let total_files = files.len();
    if total_files == 0 {
        return Err(anyhow::anyhow!("download manifest contains no files"));
    }
    let mut downloaded_files: usize = 0;
    let mut skipped_files: usize = 0;


    for (file_idx, (filename, file)) in files.into_iter().enumerate() {
        // println!("File: {:?}", file);
        let file_no = file_idx + 1;
        println!("Downloading file {}/{}: {}", file_no, total_files, filename);
        io::stdout().flush().ok();

        // Prepare final output path under .../data/<filename>, similar to EAM
        let mut out_path = out_root.to_path_buf();
        if out_path.file_name().map_or(false, |n| n == "data") == false {
            // ensure we have .../data like EAM layout
            out_path = out_path.join("data");
        }
        let out_path = out_path.join(&filename);
        if let Some(parent) = out_path.parent() { std::fs::create_dir_all(parent)?; }
        let tmp_out_path = out_path.with_extension("part");

        // Skip if final file already exists and matches expected hash/size
        let mut skip_existing = false;
        if out_path.exists() {
            // Prefer verifying by SHA1 hash if provided
            if !file.file_hash.is_empty() {
                if let Ok(mut f) = std::fs::File::open(&out_path) {
                    use std::io::Read;
                    let mut hasher = Sha1::new();
                    let mut buf = [0u8; 1024 * 1024];
                    loop {
                        match f.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => hasher.update(&buf[..n]),
                            Err(_) => break,
                        }
                    }
                    let got_hex = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();
                    if got_hex == file.file_hash {
                        println!("  skipping: existing file is up-to-date");
                        skip_existing = true;
                    }
                }
            } else {
                // Fallback: compare expected size (sum of parts)
                let expected_size: u64 = file.file_chunk_parts.iter().map(|p| p.size as u64).sum();
                if let Ok(meta) = std::fs::metadata(&out_path) {
                    if meta.len() == expected_size {
                        println!("  skipping: existing file size matches (no hash available)");
                        skip_existing = true;
                    }
                }
            }
        }
        if skip_existing {
            continue;
        }

        // 1) Ensure all required chunks are downloaded to temp as <guid>.chunk using signed links
        let total_chunks = file.file_chunk_parts.len();
        if total_chunks == 0 {
            eprintln!("Warning: zero chunk parts listed for file {}; skipping file", filename);
            skipped_files += 1;
            continue;
        }
        for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
            let guid = &part.guid;
            let chunk_path = temp_dir.join(format!("{}.chunk", guid));
            if chunk_path.exists() {
                print!("\r  chunks: {}/{} ({}%) - using cached chunk    ", chunk_idx + 1, total_chunks, ((chunk_idx + 1) * 100 / total_chunks).min(100));
                io::stdout().flush().ok();
                continue;
            }
            print!("\r  chunks: {}/{} ({}%) - downloading...        ", chunk_idx + 1, total_chunks, ((chunk_idx + 1) * 100 / total_chunks).min(100));
            io::stdout().flush().ok();
            let link = part.link.as_ref().ok_or_else(|| anyhow::anyhow!("missing signed chunk link for {}", guid))?;
            let url = link.to_string();
            // Async request with one retry
            let mut resp = client.get(url.clone()).send().await;
            if resp.is_err() { resp = client.get(url.clone()).send().await; }
            let resp = resp.map_err(|e| anyhow::anyhow!("chunk request failed for {}: {}", guid, e))?;
            let resp = resp.error_for_status().map_err(|e| anyhow::anyhow!("chunk HTTP {} for {}", e.status().unwrap_or_default(), guid))?;
            let bytes = resp.bytes().await.map_err(|e| anyhow::anyhow!("read chunk {}: {}", guid, e))?;
            std::fs::create_dir_all(chunk_path.parent().unwrap())?;
            std::fs::write(&chunk_path, &bytes)?;
        }
        println!("\r  chunks: {}/{} (100%) - done                    ", total_chunks, total_chunks);

        // 2) Reconstruct final file by reading each chunk and copying the slice [offset, offset+size)
        let mut out = std::fs::File::create(&tmp_out_path)?;
        let mut hasher = Sha1::new();
        let total_bytes: u128 = file.file_chunk_parts.iter().map(|p| p.size as u128).sum();
        let mut written: u64 = 0;
        for (chunk_idx, part) in file.file_chunk_parts.iter().enumerate() {
            let guid = &part.guid;
            let chunk_path = temp_dir.join(format!("{}.chunk", guid));
            let chunk_bytes = std::fs::read(&chunk_path)?;
            let chunk = Chunk::from_vec(chunk_bytes).ok_or_else(|| anyhow::anyhow!("failed to parse chunk {}", guid))?;
            let start = part.offset as usize;
            let end = (part.offset + part.size) as usize;
            if end > chunk.data.len() { return Err(anyhow::anyhow!("chunk too small for {} [{}..{} > {}]", filename, start, end, chunk.data.len())); }
            let slice = &chunk.data[start..end];
            std::io::Write::write_all(&mut out, slice)?;
            hasher.update(slice);
            written += part.size as u64;

            // assembly progress: chunks and MB
            let total_chunks = file.file_chunk_parts.len();
            let mb_done = (written as f64) / (1024.0 * 1024.0);
            let mb_total = (total_bytes as f64) / (1024.0 * 1024.0);
            print!("\r  assembling: {}/{} ({}%)  [{:.2} / {:.2} MB]", chunk_idx + 1, total_chunks, ((chunk_idx + 1) * 100 / total_chunks).min(100), mb_done, mb_total);
            io::stdout().flush().ok();
        }
        println!("\r  assembling: {}/{} (100%)  [{:.2} / {:.2} MB] - done", file.file_chunk_parts.len(), file.file_chunk_parts.len(), (total_bytes as f64)/(1024.0*1024.0), (total_bytes as f64)/(1024.0*1024.0));

        // Optional: verify file hash if provided (DownloadManifest uses sha1 hex)
        if !file.file_hash.is_empty() {
            let got = hasher.finalize();
            let got_hex = got.iter().map(|b| format!("{:02x}", b)).collect::<String>();
            if got_hex != file.file_hash {
                eprintln!("Warning: SHA1 mismatch for {} (expected {}, got {})", filename, file.file_hash, got_hex);
            }
        }

        // finalize atomic rename
        drop(out);
        std::fs::rename(&tmp_out_path, &out_path)?;
        downloaded_files += 1;
    }

    if downloaded_files == 0 {
        return Err(anyhow::anyhow!(format!(
            "no files could be downloaded: {} files listed, {} skipped (zero chunks)",
            total_files, skipped_files
        )));
    } else if skipped_files > 0 {
        eprintln!(
            "Note: {} of {} files were skipped due to zero chunk parts",
            skipped_files, total_files
        );
    }

    Ok(())
}

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
                                                        match download_asset(&dm, url.as_str(), &out_root).await {
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






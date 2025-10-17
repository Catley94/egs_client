//! FAB (Epic Games Fab library) endpoints.
//!
//! Handlers related to listing and refreshing the user's Fab library.

use actix_web::{get, HttpResponse};
use std::fs;
use std::io::Read;
use serde_json;

use crate::utils;

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
    let path = utils::get_fab_cache_file_path();
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
                            // println!("Using cached FAB list from {} (re-annotated)", path.display());
                        } else {
                            // println!("Using cached FAB list from {} (no changes)", path.display());
                        }
                        return HttpResponse::Ok().json(val);
                    }
                    Err(_) => {
                        // If parsing failed, fall back to returning raw bytes.
                        // println!("Using cached FAB list from {} (raw)", path.display());
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

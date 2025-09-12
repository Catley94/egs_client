//! egs_client — minimal Actix Web service to browse and download Epic Fab assets.
//!
//! What this binary does:
//! - Boots an HTTP server on 127.0.0.1:8080 (override with BIND_ADDR or PORT)
//! - Exposes routes implemented in the api module:
//!   - GET /get-fab-list: Returns cached Fab library or refreshes it.
//!   - GET /refresh-fab-list: Forces refresh from Epic Games Services (EGS).
//!   - GET /download-asset/{namespace}/{asset_id}/{artifact_id}: Downloads a specific asset.
//!
//! How to run:
//! - cargo run
//! - Visit http://127.0.0.1:8080/get-fab-list
//! - Use curl examples provided in api.rs for downloads.
//!
//! Environment and logs:
//! - Uses env_logger. To increase verbosity, run:
//!   RUST_LOG=info cargo run
//! - The server binds to 127.0.0.1:8080 by default. Override with env vars: BIND_ADDR or PORT.
//!
//! Minimal architecture diagram:
//!   main.rs (this file) -> constructs Actix App -> registers api services -> runs HttpServer
//!                              |
//!                              v
//!                         api.rs routes -> call into utils/mod.rs (auth, cache, download) -> egs_api crate

mod api;
mod utils;

use actix_web::{App, HttpServer};
use std::env;
use std::time::Duration;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize env_logger to honor RUST_LOG levels (e.g., RUST_LOG=info)
    env_logger::init();

    // Ensure runtime directories exist (non-fatal if they cannot be created)
    for dir in ["cache", "downloads"] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("Warning: failed to create directory '{}': {}", dir, e);
        }
    }

    // Determine bind address: prefer BIND_ADDR, else PORT, else 127.0.0.1:8080 (safe default for host)
    let bind_addr = if let Ok(addr) = env::var("BIND_ADDR") {
        addr
    } else if let Ok(port) = env::var("PORT") {
        format!("0.0.0.0:{}", port)
    } else {
        "127.0.0.1:8080".to_string()
    };

    println!("Starting egs_client HTTP server on {}", bind_addr);

    // Retry loop on bind failure to avoid immediate exit (e.g., short-lived port conflicts)
    loop {
        match HttpServer::new(|| {
            App::new()
                // Public HTTP endpoints
                .service(api::get_fab_list)
                .service(api::get_fab_list_post)
                .service(api::refresh_fab_list)
                .service(api::download_asset)
                .service(api::list_unreal_projects)
                .service(api::list_unreal_engines)
                .service(api::open_unreal_project)
                .service(api::open_unreal_engine)
                .service(api::import_asset)
                .service(api::create_unreal_project)
        })
        .bind(&bind_addr) {
            Ok(server) => {
                return server.run().await;
            }
            Err(e) => {
                eprintln!("Failed to bind to {}: {} — retrying in 2s...", bind_addr, e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}



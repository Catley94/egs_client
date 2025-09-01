//! egs_client — minimal Actix Web service to browse and download Epic Fab assets.
//!
//! What this binary does:
//! - Boots an HTTP server on 127.0.0.1:8080
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
//! - The server binds to 127.0.0.1:8080. Change the bind address by editing main.rs if needed.
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize env_logger to honor RUST_LOG levels (e.g., RUST_LOG=info)
    env_logger::init();

    HttpServer::new(|| {
        App::new()
            // Public HTTP endpoints
            .service(api::get_fab_list)
            .service(api::refresh_fab_list)
            .service(api::download_asset)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}



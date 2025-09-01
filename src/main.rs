mod api;
mod utils;

use actix_web::{App, HttpServer};
use std::env;



#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    // api::handle_refresh_fab_list().await;
    // In development, auto-run refresh and download without needing to hit the endpoint
    // if env::var("DEV_AUTO_REFRESH").unwrap_or_default() == "1" {
    //     // Spawn and detach so it doesn't block the HTTP server startup
    //     tokio::spawn(async {
    //         let _ = api::handle_refresh_fab_list().await;
    //     });
    // }

    HttpServer::new(|| {
        App::new()
            .service(api::get_fab_list)
            .service(api::refresh_fab_list)
            .service(api::download_asset)
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await

}



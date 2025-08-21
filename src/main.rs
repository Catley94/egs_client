mod api;
mod utils;

use egs_api::EpicGames;
use std::io::{self};
use std::time::Duration;
use actix_web::{App, HttpServer};
use egs_api::api::error::EpicAPIError;
use tokio::time::sleep;
use colored::*;



#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    HttpServer::new(|| {
        App::new()
            .service(api::get_fab_list)
            .service(api::refresh_fab_list)
    })
        .bind(("127.0.0.1:8080"))?
        .run()
        .await

}



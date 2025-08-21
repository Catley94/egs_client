mod api;
mod utils;

use actix_web::{App, HttpServer};



#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    HttpServer::new(|| {
        App::new()
            .service(api::get_fab_list)
            .service(api::refresh_fab_list)
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await

}



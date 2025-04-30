use actix_web::{App, HttpServer, web};
use env_logger;
use log::info;
use accumulator_service::{Context, api};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    info!("Starting accumulator-service HTTP server at http://127.0.0.1:8080");
    let ctx = Context::new();
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(ctx.clone()))
            .configure(api::configure)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
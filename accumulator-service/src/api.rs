use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use crate::Context;

#[derive(Deserialize)]
pub struct BuildRequest {
    parquet: String,
    resume_from: Option<String>,
}

/// POST /build
pub async fn post_build(
    ctx: web::Data<Context>,
    req: web::Json<BuildRequest>,
) -> impl Responder {
    let _ = ctx.send(crate::state_machine::Command::Build {
        parquet: req.parquet.clone(),
        resume_from: req.resume_from.clone(),
    }).await;
    HttpResponse::Accepted().finish()
}

/// GET /status
pub async fn get_status(ctx: web::Data<Context>) -> impl Responder {
    let status = ctx.status().await;
    HttpResponse::Ok().json(status)
}

/// Configure routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/build").route(web::post().to(post_build))
    )
    .service(
        web::resource("/status").route(web::get().to(get_status))
    );
}
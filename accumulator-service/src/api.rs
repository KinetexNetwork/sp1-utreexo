use crate::{
    state_machine::{Command, DispatchError},
    Context,
};
use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use std::path::PathBuf;

/// Request to start or resume a build
#[derive(Deserialize)]
pub struct BuildRequest {
    pub parquet: String,
    pub resume_from: Option<String>,
}

/// POST /build
pub async fn post_build(ctx: web::Data<Context>, req: web::Json<BuildRequest>) -> impl Responder {
    match ctx
        .send(Command::Build {
            parquet: req.parquet.clone(),
            resume_from: req.resume_from.clone(),
        })
        .await
    {
        Ok(_) => HttpResponse::Accepted().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// GET /status
pub async fn get_status(ctx: web::Data<Context>) -> impl Responder {
    let status = ctx.status().await;
    HttpResponse::Ok().json(status)
}

/// POST /pause
pub async fn post_pause(ctx: web::Data<Context>) -> impl Responder {
    match ctx.send(Command::Pause).await {
        Ok(_) => HttpResponse::Accepted().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// POST /resume
pub async fn post_resume(ctx: web::Data<Context>) -> impl Responder {
    match ctx.send(Command::Resume).await {
        Ok(_) => HttpResponse::Accepted().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// POST /stop
pub async fn post_stop(ctx: web::Data<Context>) -> impl Responder {
    match ctx.send(Command::Stop).await {
        Ok(_) => HttpResponse::Accepted().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// Request to update a single block
#[derive(Deserialize)]
pub struct UpdateRequest {
    pub height: u64,
}
/// POST /update
pub async fn post_update(ctx: web::Data<Context>, req: web::Json<UpdateRequest>) -> impl Responder {
    match ctx.send(Command::Update(req.height)).await {
        Ok(_) => HttpResponse::Accepted().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// POST /dump: trigger pollard prune and return 202 Accepted
pub async fn post_dump(ctx: web::Data<Context>) -> impl Responder {
    // default dump directory "snapshot" inside current working directory
    match ctx
        .send(Command::Dump {
            dir: PathBuf::from("snapshot"),
        })
        .await
    {
        Ok(_) => HttpResponse::Accepted().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// POST /restore: trigger service to reload state from disk
pub async fn post_restore(ctx: web::Data<Context>) -> impl Responder {
    match ctx
        .send(Command::Restore {
            dir: PathBuf::from("snapshot"),
        })
        .await
    {
        Ok(_) => HttpResponse::Created().finish(),
        Err(DispatchError::InvalidState) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// Configure routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/build").route(web::post().to(post_build)))
        .service(web::resource("/pause").route(web::post().to(post_pause)))
        .service(web::resource("/resume").route(web::post().to(post_resume)))
        .service(web::resource("/stop").route(web::post().to(post_stop)))
        .service(web::resource("/update").route(web::post().to(post_update)))
        .service(web::resource("/dump").route(web::post().to(post_dump)))
        .service(web::resource("/restore").route(web::post().to(post_restore)))
        .service(web::resource("/status").route(web::get().to(get_status)));
}

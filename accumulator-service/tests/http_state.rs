//! Integration test using Actix `test` harness without touching the network.

use accumulator_service::{api, Context};
use actix_web::{test, web, App};
use serde_json::json;

#[actix_rt::test]
async fn start_build_then_conflict_on_second_build() {
    // temp dir for working directory so we do not touch real fs
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(&tmp).unwrap();

    let ctx = Context::new();

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.clone()))
            .configure(api::configure),
    )
    .await;

    // First /build should be 202 Accepted
    let req1 = test::TestRequest::post()
        .uri("/build")
        .set_json(&json!({ "parquet": "nonexistent.parquet", "resume_from": null }))
        .to_request();
    let resp1 = test::call_service(&app, req1).await;
    assert_eq!(resp1.status(), 202);

    // Second /build while first still running should yield 409 Conflict
    let req2 = test::TestRequest::post()
        .uri("/build")
        .set_json(&json!({ "parquet": "other.parquet", "resume_from": null }))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 409);

    // /status should return error eventually (because file missing) but at least state not Idle
    #[allow(clippy::let_underscore_future)]
    {
        let req_status = test::TestRequest::get().uri("/status").to_request();
        let resp_status = test::call_service(&app, req_status).await;
        assert_eq!(resp_status.status(), 200);
    }
}

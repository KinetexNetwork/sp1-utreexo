//! Integration test: POST /update should rebuild & dump pruned Pollard

use accumulator_service::{api, Context};
use actix_web::{test, web::Data, App};
use serde_json::json;
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use std::fs::File;
use std::path::Path;
use std::time::Duration;

/// Wait until service state returns Idle
async fn wait_idle(ctx: &Context) {
    loop {
        let status = ctx.status().await;
        if matches!(status.state, accumulator_service::state_machine::ServiceState::Idle) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[actix_rt::test]
async fn update_generates_pollard_bin() {
    // isolate in temp dir
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    // prepare minimal mem_forest.bin (empty forest)
    let forest: MemForest<BitcoinNodeHash> = MemForest::new();
    let mut f = File::create("mem_forest.bin").unwrap();
    forest.serialize(&mut f).unwrap();

    // ensure no pollard.bin present
    assert!(!Path::new("pollard.bin").exists());

    // start service
    let ctx = Context::new();
    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.clone()))
            .configure(api::configure),
    )
    .await;

    // POST /update (height=0), expect 202
    let req = test::TestRequest::post()
        .uri("/update")
        .set_json(&json!({ "height": 0 }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 202);

    // wait for pollard.bin to be written
    for _ in 0..20 {
        if Path::new("pollard.bin").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // pollard.bin should now exist
    assert!(Path::new("pollard.bin").exists(), "pollard.bin not created");
}
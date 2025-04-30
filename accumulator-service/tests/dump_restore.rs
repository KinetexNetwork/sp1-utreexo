//! Integration-ish tests for the Dump / Restore implementation (phase-A).

use accumulator_service::state_machine::{Command, Context, ServiceState};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use std::fs::File;
use std::io::Write;

async fn wait_until_idle(ctx: &Context) {
    loop {
        if matches!(ctx.status().await.state, ServiceState::Idle) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn dump_and_restore_roundtrip() {
    let workdir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(&workdir).unwrap();

    // create minimal mem_forest.bin
    let forest: MemForest<BitcoinNodeHash> = MemForest::new();
    let mut f = File::create("mem_forest.bin").unwrap();
    forest.serialize(&mut f).unwrap();

    // create context & issue dump
    let ctx = Context::new();
    let snapshot_dir = workdir.path().join("snap");
    ctx.send(Command::Dump { dir: snapshot_dir.clone() })
        .await
        .unwrap();

    // Wait until mem_forest.bin appears in snapshot dir (dump finished)
    for _ in 0..20 {
        if snapshot_dir.join("mem_forest.bin").exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(snapshot_dir.join("mem_forest.bin").exists());
    assert!(snapshot_dir.join("pollard.bin").exists());

    // ensure dump task reported Idle
    wait_until_idle(&ctx).await;

    // corrupt working dir mem_forest.bin
    File::create("mem_forest.bin").unwrap().write_all(b"corrupt").unwrap();

    // restore
    ctx.send(Command::Restore { dir: snapshot_dir.clone() })
        .await
        .unwrap();
    wait_until_idle(&ctx).await;

    // after restore mem_forest.bin contents should equal snapshot copy
    let orig = std::fs::read(snapshot_dir.join("mem_forest.bin")).unwrap();
    let new = std::fs::read("mem_forest.bin").unwrap();
    assert_eq!(orig, new);
}

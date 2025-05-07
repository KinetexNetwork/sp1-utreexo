//! Integration-ish tests for the Dump / Restore implementation (phase-A).

use accumulator_service::state_machine::{Command, Context, ServiceState};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use std::fs::File;

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

    // touch block_hashes.bin to ensure it is included in snapshot
    std::fs::write("block_hashes.bin", b"dummy").unwrap();

    // create context & issue dump
    let ctx = Context::new();
    let snapshot_dir = workdir.path().join("snap");
    ctx.send(Command::Dump {
        dir: snapshot_dir.clone(),
    })
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
    assert!(snapshot_dir.join("block_hashes.bin").exists());

    // ensure dump task reported Idle
    wait_until_idle(&ctx).await;

    // remove mem_forest.bin to simulate missing/invalid state
    std::fs::remove_file("mem_forest.bin").unwrap();

    // restore
    ctx.send(Command::Restore {
        dir: snapshot_dir.clone(),
    })
    .await
    .unwrap();
    wait_until_idle(&ctx).await;

    // after restore mem_forest.bin contents should equal snapshot copy
    for f in ["mem_forest.bin", "block_hashes.bin"].iter() {
        let orig = std::fs::read(snapshot_dir.join(f)).unwrap();
        let new = std::fs::read(f).unwrap();
        assert_eq!(orig, new, "{} differs after restore", f);
    }
}

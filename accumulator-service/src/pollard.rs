//! Pollard logic stub.
use anyhow::Result;

/// Prune a full MemForest snapshot at `snapshot_path` using delete hashes file.
pub async fn prune_forest(snapshot_path: &str, delete_list: &str) -> Result<()> {
    println!("Stub: prune_forest from {} with {}", snapshot_path, delete_list);
    Ok(())
}
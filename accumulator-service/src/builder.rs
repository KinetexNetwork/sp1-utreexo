use crate::script_utils::parquet::get_all_leaf_hashes;
/// Builder logic: load leaf hashes from Parquet, build or resume a MemForest, and serialize it.
use anyhow::{Context, Result};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use std::fs::File;

/// Start building the accumulator from a Parquet dump, optionally resuming from an existing snapshot.
/// On success writes out `mem_forest.bin` in the current directory.
pub async fn start_build(parquet: &str, resume_from: Option<&str>) -> Result<()> {
    // Load existing forest or create new
    let mut forest: MemForest<BitcoinNodeHash> = if let Some(path) = resume_from {
        let mut f = File::open(path).with_context(|| format!("failed to open snapshot: {path}"))?;
        MemForest::deserialize(&mut f).context("failed to deserialize existing MemForest")?
    } else {
        MemForest::new()
    };
    // Extract all leaf hashes from the Parquet file
    let leaves = get_all_leaf_hashes(parquet)
        .with_context(|| format!("failed to extract leaf hashes from {parquet}"))?;
    // Apply all leaves as additions (initial build)
    forest
        .modify(&leaves, &[])
        .map_err(|e| anyhow::anyhow!("failed to insert leaves into MemForest: {}", e))?;
    // Serialize the updated forest to disk
    let mut out = File::create("mem_forest.bin").context("failed to create mem_forest.bin")?;
    forest
        .serialize(&mut out)
        .context("failed to serialize MemForest")?;
    Ok(())
}

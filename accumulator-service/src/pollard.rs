//! Pollard logic stubs and helpers
use crate::script_utils::pollard_conv::forest_to_pollard;
use anyhow::{anyhow, Context, Result};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::{Pollard, PollardAddition};
use rustreexo::accumulator::proof::Proof;
use std::fs;
use std::io::Cursor;

/// Prune a MemForest snapshot into a Pollard using the provided delete list (ignored for empty deletions).
/// Reads the serialized MemForest from `snapshot_path`, runs the forest_to_pollard conversion,
/// and writes out `pollard.bin` in the current directory.
pub async fn prune_forest(snapshot_path: &str, _delete_list: &str) -> Result<()> {
    // Load the full MemForest bytes
    let data = fs::read(snapshot_path)?;
    // Convert to Pollard (empty deletions by default)
    let pollard =
        forest_to_pollard(&data, &[]).map_err(|e| anyhow!("pollard conversion failed: {}", e))?;
    // Serialize Pollard to disk
    let mut out = fs::File::create("pollard.bin")?;
    pollard
        .serialize(&mut out)
        .map_err(|e| anyhow!("failed to serialize Pollard: {}", e))?;
    Ok(())
}
/// Synchronous version of prune_forest for use in blocking contexts.
pub fn prune_forest_sync(snapshot_path: &str, _delete_list: &str) -> Result<()> {
    // Load the full MemForest bytes
    let data = fs::read(snapshot_path)?;
    // Convert to Pollard (empty deletions by default)
    let pollard =
        forest_to_pollard(&data, &[]).map_err(|e| anyhow!("pollard conversion failed: {}", e))?;
    // Serialize Pollard to disk
    let mut out = fs::File::create("pollard.bin")?;
    pollard
        .serialize(&mut out)
        .map_err(|e| anyhow!("failed to serialize Pollard: {}", e))?;
    Ok(())
}

// ----------------------------------------------------------------------------
// Build an in-memory Pollard reflecting a single block's deletes and additions
// ----------------------------------------------------------------------------

/// Apply a block's deletes (TxIns) and new leaves (TxOuts) to a full `MemForest` in memory,
/// producing a compact `Pollard` that matches the resulting accumulator root.
///
/// Steps:
/// 1) Deserialize `mem_forest_bytes` into a `MemForest`.
/// 2) Generate a batch proof for `deletes` (spent leaf hashes).
/// 3) Create a fresh `Pollard` from the forest's roots + leaf count; call `modify` with the proof.
/// 4) Append every hash in `new_leaves` as `PollardAddition`.
/// 5) Apply the same `(new_leaves, deletes)` to the full `MemForest` and assert roots match.
pub fn pollard_after_block(
    mem_forest_bytes: &[u8],
    deletes: &[BitcoinNodeHash],
    new_leaves: &[BitcoinNodeHash],
) -> Result<Pollard<BitcoinNodeHash>> {
    // 1) deserialize full forest
    let mut cursor = Cursor::new(mem_forest_bytes);
    let mut mem = MemForest::<BitcoinNodeHash>::deserialize(&mut cursor)
        .context("deserialize MemForest failed")?;

    // 2) build deletion proof
    let proof: Proof<BitcoinNodeHash> = mem
        .prove(deletes)
        .map_err(|e| anyhow!("prove failed: {e:?}"))?;

    // 3) create Pollard from current roots and apply proof + additions
    let roots = mem
        .get_roots()
        .iter()
        .map(|r| r.get_data())
        .collect::<Vec<_>>();
    let adds = new_leaves
        .iter()
        .map(|&h| PollardAddition {
            hash: h,
            remember: false,
        })
        .collect::<Vec<_>>();
    let mut pollard = Pollard::from_roots(roots, mem.leaves);
    pollard
        .modify(&adds, deletes, proof)
        .map_err(|e| anyhow!("pollard.modify failed: {e}"))?;

    // 4) sanity check: mirror on MemForest and compare roots
    mem.modify(new_leaves, deletes)
        .map_err(|e| anyhow!("mem.modify failed: {e:?}"))?;
    let expected = mem
        .get_roots()
        .iter()
        .map(|r| r.get_data())
        .collect::<Vec<_>>();
    if pollard.roots() != expected {
        return Err(anyhow!("root mismatch: Pollard vs MemForest after block"));
    }

    Ok(pollard)
}

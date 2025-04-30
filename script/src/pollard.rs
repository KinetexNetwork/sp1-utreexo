//! Library for converting a serialized MemForest into a pruned Pollard given delete hashes.
use anyhow::{Context, Result};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::io::Cursor;

/// Take a serialized MemForest (as bytes) and a list of leaf hashes to delete,
/// then return a Pollard pruned to those leaves.
pub fn forest_to_pollard(
    bytes: &[u8],
    delete_hashes: &[BitcoinNodeHash],
) -> Result<Pollard<BitcoinNodeHash>> {
    // Deserialize the MemForest
    let mut cursor = Cursor::new(bytes);
    let mem_forest = MemForest::<BitcoinNodeHash>::deserialize(&mut cursor)
        .context("failed to deserialize MemForest")?;
    // Obtain the proof for deletion
    let proof = mem_forest
        .prove(delete_hashes)
        .map_err(|e| anyhow::anyhow!("prove failed: {:?}", e))?;
    let remember = proof.targets.clone();
    // Build Pollard skeleton from roots
    let roots = mem_forest
        .get_roots()
        .iter()
        .map(|r| r.get_data())
        .collect::<Vec<_>>();
    let mut pollard = Pollard::from_roots(roots, mem_forest.leaves);
    // Ingest proof
    pollard
        .ingest_proof(proof, delete_hashes, &remember)
        .map_err(|e| anyhow::anyhow!("ingest proof failed: {:?}", e))?;
    Ok(pollard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustreexo::accumulator::node_hash::BitcoinNodeHash;

    #[test]
    fn unknown_leaf_errors() {
        // empty bytes cannot deserialize => error
        let hash = BitcoinNodeHash::default();
        assert!(forest_to_pollard(&[], &[hash]).is_err());
    }
}

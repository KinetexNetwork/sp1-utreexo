//! Library for fetching inputs of a block as Utreexo leaf hashes via a Bitcoin RPC interface.
use anyhow::{Context, Result};
use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, TxOut};
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use utreexo::LeafData;

/// Abstract RPC interface for fetching blocks and UTXOs.
pub trait BitcoinRpc {
    /// Given a block height, return its BlockHash.
    fn get_block_hash(&self, height: u64) -> Result<BlockHash>;
    /// Given a block hash, return the full Block.
    fn get_block(&self, hash: &BlockHash) -> Result<bitcoin::Block>;
    /// Given an outpoint, return (value in satoshis, raw script bytes).
    fn get_txout(&self, prevout: &OutPoint) -> Result<(u64, Vec<u8>)>;
    /// Given a block hash, return its confirmed height.
    fn get_block_height(&self, hash: &BlockHash) -> Result<u32>;
}

/// Fetch all non-coinbase inputs of a block as Utreexo leaf hashes.
/// Returns one leaf hash per spent UTXO input in the block.
pub fn get_block_leaf_hashes<R: BitcoinRpc>(
    rpc: &R,
    block_height: u64,
) -> Result<Vec<BitcoinNodeHash>> {
    // find the block by height
    let block_hash = rpc
        .get_block_hash(block_height)
        .with_context(|| format!("failed to get block hash at height {}", block_height))?;
    let block = rpc
        .get_block(&block_hash)
        .with_context(|| format!("failed to fetch block {}", block_hash))?;
    let height = rpc
        .get_block_height(&block_hash)
        .with_context(|| format!("failed to fetch header for block {}", block_hash))?;
    let mut hashes = Vec::new();
    for tx in block.txdata.into_iter() {
        if tx.is_coinbase() {
            continue;
        }
        for txin in tx.input.into_iter() {
            let prev = txin.previous_output;
            let (value, script_bytes) = rpc
                .get_txout(&prev)
                .with_context(|| format!("failed to fetch UTXO {:?}", prev))?;
            let utxo = TxOut {
                value: Amount::from_sat(value),
                script_pubkey: ScriptBuf::from_bytes(script_bytes),
            };
            let header_code = height << 1;
            let leaf = LeafData {
                block_hash,
                prevout: prev,
                header_code,
                utxo,
            };
            hashes.push(leaf.get_leaf_hashes());
        }
    }
    Ok(hashes)
}

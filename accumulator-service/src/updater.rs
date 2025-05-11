//! Updater logic: fetch spent UTXO leaf hashes from a block via RPC and apply deletions to the MemForest snapshot.
use crate::script_utils::btc_rpc::{get_block_leaf_hashes, BitcoinRpc};
use anyhow::{anyhow, Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use std::env;
use std::fs::File;

/// RPC wrapper for the BitcoinRpc trait using bitcoincore_rpc::Client.
struct RpcClient(Client);
impl BitcoinRpc for RpcClient {
    fn get_block_hash(&self, height: u64) -> Result<bitcoin::BlockHash> {
        Ok(self.0.get_block_hash(height)?)
    }
    fn get_block(&self, hash: &bitcoin::BlockHash) -> Result<bitcoin::Block> {
        Ok(self.0.get_block(hash)?)
    }
    fn get_txout(&self, prevout: &bitcoin::OutPoint) -> Result<(u64, Vec<u8>)> {
        let info = self.0.get_raw_transaction_info(&prevout.txid, None)?;
        let out = info
            .vout
            .into_iter()
            .find(|v| v.n == prevout.vout)
            .context("vout not found")?;
        Ok((out.value.to_sat(), hex::decode(out.script_pub_key.hex)?))
    }
    fn get_block_height(&self, hash: &bitcoin::BlockHash) -> Result<u32> {
        let hdr = self.0.get_block_header_info(hash)?;
        Ok(hdr.height as u32)
    }
}

/// Update the accumulator by deleting all spent UTXO leaves in block `height`.
pub async fn update_block(height: u64) -> Result<()> {
    // Determine delete list: try Bitcoin RPC if env vars set, else default to empty
    let deletes = if let (Ok(rpc_url), Ok(cookie)) = (
        env::var("BITCOIN_CORE_RPC_URL"),
        env::var("BITCOIN_CORE_COOKIE_FILE"),
    ) {
        if let Ok(client) = Client::new(&rpc_url, Auth::CookieFile(cookie.into())) {
            let rpc = RpcClient(client);
            get_block_leaf_hashes(&rpc, height).unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    // Load existing MemForest snapshot
    let mut f = File::open("mem_forest.bin").context("failed to open mem_forest.bin")?;
    let mut forest = MemForest::<BitcoinNodeHash>::deserialize(&mut f)
        .context("failed to deserialize MemForest")?;

    // Apply deletions
    forest
        .modify(&[], &deletes)
        .map_err(|e| anyhow!("failed to delete leaves in MemForest: {}", e))?;

    // Serialize updated forest
    let mut out =
        File::create("mem_forest.bin").context("failed to open mem_forest.bin for write")?;
    forest
        .serialize(&mut out)
        .context("failed to serialize MemForest")?;
    // After updating the forest, generate a fresh pruned Pollard and write pollard.bin
    // offload pruning to blocking thread since Pollard sync conversion is not Send-safe
    tokio::task::spawn_blocking(|| crate::pollard::prune_forest_sync("mem_forest.bin", ""))
        .await
        .context("prune_forest task join failed")?
        .context("failed to prune forest to Pollard")?;
    Ok(())
}
/// Synchronous helper for `update_block`, suitable for blocking contexts.
pub fn update_block_sync(height: u64) -> Result<()> {
    // Build a local runtime and execute the async update
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create runtime for update_block_sync")?;
    rt.block_on(update_block(height))
        .context("error running update_block")
}

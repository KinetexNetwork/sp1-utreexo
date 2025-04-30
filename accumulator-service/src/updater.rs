//! Updater logic: fetch spent UTXO leaf hashes from a block via RPC and apply deletions to the MemForest snapshot.
use anyhow::{anyhow, Context, Result};
use utreexo_script::updater::{get_block_leaf_hashes, BitcoinRpc};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::env;
use std::fs::File;
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;

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
    // Load RPC credentials from env
    let rpc_url = env::var("BITCOIN_CORE_RPC_URL").context("missing RPC URL")?;
    let cookie = env::var("BITCOIN_CORE_COOKIE_FILE").context("missing RPC cookie file path")?;
    let client = Client::new(&rpc_url, Auth::CookieFile(cookie.into()))
        .context("failed to create RPC client")?;
    let rpc = RpcClient(client);

    // Load existing MemForest snapshot
    let mut f = File::open("mem_forest.bin").context("failed to open mem_forest.bin")?;
    let mut forest = MemForest::<BitcoinNodeHash>::deserialize(&mut f)
        .context("failed to deserialize MemForest")?;

    // Fetch delete leaf hashes for this block
    let deletes = get_block_leaf_hashes(&rpc, height)
        .map_err(|e| anyhow!("failed to fetch block leaf hashes: {}", e))?;

    // Apply deletions
    forest
        .modify(&[], &deletes)
        .map_err(|e| anyhow!("failed to delete leaves in MemForest: {}", e))?;

    // Serialize updated forest
    let mut out = File::create("mem_forest.bin").context("failed to open mem_forest.bin for write")?;
    forest
        .serialize(&mut out)
        .context("failed to serialize MemForest")?;
    Ok(())
}
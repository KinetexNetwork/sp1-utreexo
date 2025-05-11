//! Standalone verifier: loads a pruned Pollard, fetches block H and H+1,
//! and applies UTXO changes to advance the Pollard.
use anyhow::{anyhow, Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use clap::Parser;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::{Pollard, PollardAddition};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::proof::Proof;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use accumulator_service::script_utils::btc_rpc::{get_block_leaf_hashes, BitcoinRpc};
use utreexo::LeafData;

/// CLI arguments
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Path to the pruned Pollard file (pollard.bin)
    #[arg(long)]
    pollard: PathBuf,
    /// Block height H to process updates for H and H+1
    #[arg(long)]
    height: u64,
}

fn main() -> Result<()> {
    // Parse arguments
    let args = Args::parse();

    // (1) Load existing pruned Pollard
    let mut pollard_bytes = Vec::new();
    File::open(&args.pollard)
        .with_context(|| format!("opening pollard file {:?}", args.pollard))?
        .read_to_end(&mut pollard_bytes)?;
    let mut rdr = Cursor::new(&pollard_bytes);
    let mut pollard: Pollard<BitcoinNodeHash> =
        Pollard::deserialize(&mut rdr).context("failed to deserialize pollard")?;
    let prev_roots = pollard.roots().to_vec();
    println!("Previous Utreexo roots: {:?}", prev_roots);

    // (2) Connect to local Bitcoin Core RPC
    let rpc_url = std::env::var("BITCOIN_CORE_RPC_URL").context("missing BITCOIN_CORE_RPC_URL")?;
    let cookie = std::env::var("BITCOIN_CORE_COOKIE_FILE").context("missing BITCOIN_CORE_COOKIE_FILE")?;
    let rpc_client = Client::new(&rpc_url, Auth::CookieFile(cookie.into()))
        .context("failed to connect to Bitcoin RPC")?;
    let rpc = RpcClient(rpc_client);

    // (3) Fetch block H and H+1
    let bh0 = rpc.get_block_hash(args.height)?;
    let block0 = rpc.get_block(&bh0)?;
    let h1 = args.height + 1;
    let bh1 = rpc.get_block_hash(h1)?;
    let block1 = rpc.get_block(&bh1)?;
    println!("Block {} hash = {}", args.height, bh0);
    println!("Block {} hash = {}", h1, bh1);

    // (4) Verify difficulty target matches between blocks
    if block0.header.bits != block1.header.bits {
        eprintln!("Warning: bits mismatch: {:?} vs {:?}", block0.header.bits, block1.header.bits);
    }

    // (5) Compute deletes (spent UTXO leaves) for block H+1
    let deletes = get_block_leaf_hashes(&rpc, h1)
        .context("failed to fetch block leaf hashes")?;
    println!("Deletes from block {}: {} leaves", h1, deletes.len());

    // (6) Compute adds (new UTXO leaves) from block H+1
    let height_code = rpc.get_block_height(&bh1).context("fetch block height")? << 1;
    let mut adds = Vec::new();
    for tx in &block1.txdata {
        for (vout, out) in tx.output.iter().enumerate() {
            let leaf_data = LeafData {
                block_hash: bh1,
                prevout: bitcoin::OutPoint { txid: tx.txid(), vout: vout as u32 },
                header_code: height_code,
                utxo: out.clone(),
            };
            adds.push(PollardAddition { hash: leaf_data.get_leaf_hashes(), remember: false });
        }
    }
    println!("Adds from block {}: {} leaves", h1, adds.len());

    // (7) Load full MemForest to generate an update proof
    let mut forest_bytes = Vec::new();
    File::open("mem_forest.bin").context("opening mem_forest.bin")?
        .read_to_end(&mut forest_bytes)?;
    let mut fcur = Cursor::new(&forest_bytes);
    let mut forest: MemForest<BitcoinNodeHash> =
        MemForest::deserialize(&mut fcur).context("deserialize forest")?;
    let proof: Proof<BitcoinNodeHash> = forest
        .prove(&deletes)
        .map_err(|e| anyhow!("prove failed: {:?}", e))?;

    // (8) Apply add/delete/proof to the pruned Pollard
    pollard
        .modify(&adds, &deletes, proof)
        .map_err(|e| anyhow!("pollard.modify failed: {:?}", e))?;
    let new_roots = pollard.roots().to_vec();
    println!("New Utreexo roots: {:?}", new_roots);

    // (9) Output commit values
    println!("Commit:");
    println!("- prev_block_hash = {}", bh0);
    println!("- prev_utreexo_roots = {:?}", prev_roots);
    println!("- block_hash = {}", bh1);
    println!("- new_utreexo_roots = {:?}", new_roots);
    Ok(())
}

/// A thin RPCAdapter implementing BitcoinRpc for our usage
struct RpcClient(Client);
impl BitcoinRpc for RpcClient {
    fn get_block_hash(&self, height: u64) -> Result<bitcoin::BlockHash> {
        Ok(self.0.get_block_hash(height)?)
    }
    fn get_block(&self, hash: &bitcoin::BlockHash) -> Result<bitcoin::Block> {
        Ok(self.0.get_block(hash)?)
    }
    fn get_txout(&self, prev: &bitcoin::OutPoint) -> Result<(u64, Vec<u8>)> {
        let info = self.0.get_raw_transaction_info(&prev.txid, None)?;
        let v = info.vout.into_iter().find(|v| v.n == prev.vout)
            .ok_or_else(|| anyhow!("vout not found"))?;
        Ok((v.value.to_sat(), hex::decode(v.script_pub_key.hex)?))
    }
    fn get_block_height(&self, hash: &bitcoin::BlockHash) -> Result<u32> {
        Ok(self.0.get_block_header_info(hash)?.height as u32)
    }
}
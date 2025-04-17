use std::{
    env,
    fs::{create_dir_all, File},
    io::Write,
    path::PathBuf,
    time::Instant,
};

use anyhow::Result;
use bitcoin::{blockdata::script::ScriptBuf, Amount, OutPoint, TxOut, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use duckdb::Connection;
use humantime::format_duration;
use log::info;
use num_format::{Locale, ToFormattedString};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use utreexo::btc_structs::LeafData;

fn main() -> Result<()> {
    env_logger::init();

    // args: <parquet> <out_dir>
    let parquet = env::args()
        .nth(1)
        .expect("Usage: utreexo‐script <utxo.parquet> <out_dir>");
    let out_dir = PathBuf::from(env::args().nth(2).unwrap());
    create_dir_all(&out_dir)?;

    info!("Script started");

    // rpc setup (cookie‐file auth)
    let rpc_url = env::var("BITCOIN_CORE_RPC_URL").expect("set BITCOIN_CORE_RPC_URL env var");
    let cookie_file =
        env::var("BITCOIN_CORE_COOKIE_FILE").expect("set BITCOIN_CORE_COOKIE_FILE env var");
    let rpc = Client::new(&rpc_url, Auth::CookieFile(PathBuf::from(cookie_file)))?;

    // Prefetch all block‐hashes
    let t_fetch_start = Instant::now();
    let tip_height = rpc.get_block_count()? as usize;
    let mut block_hashes = Vec::with_capacity(tip_height + 1);
    for h in 0..=tip_height {
        block_hashes.push(rpc.get_block_hash(h as u64)?);
    }
    let fetch_dur = t_fetch_start.elapsed();
    info!(
        "Fetched {} block hashes in {:?}",
        block_hashes.len().to_formatted_string(&Locale::en),
        format_duration(fetch_dur)
    );

    // Open an in-memory DB over your Parquet file
    let conn = Connection::open(":memory:")?;
    let batch_size = 50_000;
    let mut offset = 0;
    let mut batch_idx = 0;

    // Our utreexo accumulator
    let mut mem_forest = MemForest::<BitcoinNodeHash>::new();

    loop {
        let loop_start = Instant::now();
        // Adjust this SELECT to match your Parquet schema from dumptxoutset:
        let sql = format!(
            r#"
            SELECT txid, amount, vout, height, script
             FROM '{}'
             WHERE coinbase = FALSE
             LIMIT {} OFFSET {}
        "#,
            parquet, batch_size, offset
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut leaves = Vec::with_capacity(batch_size);

        for row in stmt.query_map([], |r| {
            let txid_hex: String = r.get(0)?;
            let amount: u64 = r.get(1)?;
            let vout: u32 = r.get(2)?;
            let height: u64 = r.get(3)?;
            let script_bytes: Vec<u8> = r.get(4)?;
            Ok((txid_hex, amount, vout, height, script_bytes))
        })? {
            let (txid_hex, sats, vout, height, script_bytes) = row?;
            let txid: Txid = txid_hex.parse()?;
            let block_hash = block_hashes[height as usize];
            let script_pubkey = ScriptBuf::from_bytes(script_bytes);
            let header_code = (height as u32) << 1;

            let txout = TxOut {
                value: Amount::from_sat(sats),
                script_pubkey,
            };

            let leaf = LeafData {
                block_hash,
                prevout: OutPoint { txid, vout },
                header_code,
                utxo: txout,
            };

            leaves.push(leaf.get_leaf_hashes());
        }

        if leaves.is_empty() {
            break;
        }

        mem_forest.modify(&leaves, &[]).unwrap();
        let loop_dur = loop_start.elapsed();
        info!(
            "Batch {}: processed {} leaves in {:?} (offset {})",
            batch_idx.to_formatted_string(&Locale::en),
            leaves.len().to_formatted_string(&Locale::en),
            loop_dur,
            offset.to_formatted_string(&Locale::en)
        );
        offset += batch_size;
        batch_idx += 1;
    }

    let processing_time = t_fetch_start.elapsed();

    info!("{} batches processed for {:?}", batch_idx, processing_time);

    // Dump block_hashes.bin
    let mut flat = Vec::with_capacity(block_hashes.len() * 32);
    for h in &block_hashes {
        flat.extend_from_slice(h.as_ref());
    }
    File::create(out_dir.join("block_hashes.bin"))?.write_all(&flat)?;

    // Dump mem_forest.bin
    let mut f = File::create(out_dir.join("mem_forest.bin"))?;
    mem_forest.serialize(&mut f)?;

    Ok(())
}

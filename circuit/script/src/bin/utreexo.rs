use std::{
    env,
    fs::{create_dir_all, File},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::Result;
use bitcoin::{blockdata::script::ScriptBuf, Amount, BlockHash, OutPoint, TxOut, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use duckdb::Connection;
use log::info;
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use utreexo::btc_structs::LeafData;

/// build the SQL we use for ONE batch
fn build_sql_query(parquet: &str, limit: usize, offset: usize) -> String {
    format!(
        "SELECT txid, amount, vout, height, script \
         FROM '{}' \
         WHERE coinbase = FALSE \
         LIMIT {} OFFSET {}",
        parquet, limit, offset
    )
}

/// Dump all block‐hashes to `path` as 32‐byte LE‐concatenated blobs
fn dump_block_hashes(hashes: &[BlockHash], path: &Path) -> Result<()> {
    let mut flat = Vec::with_capacity(hashes.len() * 32);
    for h in hashes {
        flat.extend_from_slice(h.as_ref());
    }
    let mut f = File::create(path)?;
    f.write_all(&flat)?;
    Ok(())
}

/// Serialize the accumulator out to a file
fn dump_mem_forest(forest: &MemForest<BitcoinNodeHash>, path: &Path) -> Result<()> {
    let mut f = File::create(path)?;
    forest.serialize(&mut f)?;
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();

    // args
    let parquet = env::args()
        .nth(1)
        .expect("Usage: utreexo <utxo.parquet> <out_dir>");
    let out_dir = PathBuf::from(env::args().nth(2).unwrap());
    create_dir_all(&out_dir)?;

    // RPC client
    let rpc_url = env::var("BITCOIN_CORE_RPC_URL").expect("…RPC_URL…");
    let cookie_file = env::var("BITCOIN_CORE_COOKIE_FILE").expect("…COOKIE_FILE…");
    let rpc = Client::new(&rpc_url, Auth::CookieFile(PathBuf::from(cookie_file)))?;

    // fetch all block‐hashes
    let t0 = Instant::now();
    let tip = rpc.get_block_count()? as usize;
    let mut block_hashes = Vec::with_capacity(tip + 1);
    for h in 0..=tip {
        block_hashes.push(rpc.get_block_hash(h as u64)?);
    }
    info!(
        "fetched {} block‐hashes in {:?}",
        block_hashes.len(),
        t0.elapsed()
    );

    // open Parquet in memory
    let conn = Connection::open(":memory:")?;
    let batch_size = 50_000;
    let mut offset = 0;
    let mut forest = MemForest::<BitcoinNodeHash>::new();
    let mut batch_idx = 0;

    loop {
        let t1 = Instant::now();
        let sql = build_sql_query(&parquet, batch_size, offset);
        let mut stmt = conn.prepare(&sql)?;
        let mut leaves = Vec::with_capacity(batch_size);

        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, Vec<u8>>(4)?,
            ))
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
        forest.modify(&leaves, &[]).unwrap();
        info!(
            "batch {}: {} leaves in {:?} (offset {})",
            batch_idx,
            leaves.len(),
            t1.elapsed(),
            offset
        );
        offset += batch_size;
        batch_idx += 1;
    }

    // write out
    dump_block_hashes(&block_hashes, &out_dir.join("block_hashes.bin"))?;
    dump_mem_forest(&forest, &out_dir.join("mem_forest.bin"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn sql_builder() {
        let got = build_sql_query("foo.parquet", 123, 456);
        let want = "SELECT txid, amount, vout, height, script FROM 'foo.parquet' \
                    WHERE coinbase = FALSE \
                    LIMIT 123 OFFSET 456";
        assert_eq!(got, want);
    }

    #[test]
    fn dump_block_hashes_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blocks.bin");
        let h1 = BlockHash::from_slice(&[1u8; 32]).unwrap();
        let h2 = BlockHash::from_slice(&[2u8; 32]).unwrap();
        dump_block_hashes(&[h1, h2], &path).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[0..32], &[1u8; 32]);
        assert_eq!(&data[32..64], &[2u8; 32]);
    }

    #[test]
    fn dump_mem_forest_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("forest.bin");
        let forest = MemForest::<BitcoinNodeHash>::new();
        // empty forest → serialize/deserialize
        dump_mem_forest(&forest, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut cur = Cursor::new(bytes);
        let forest2 = MemForest::<BitcoinNodeHash>::deserialize(&mut cur).unwrap();
        assert_eq!(forest.get_roots().len(), forest2.get_roots().len());
    }
}

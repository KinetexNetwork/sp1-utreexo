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
use humantime::format_duration;
use log::info;
use num_format::{Locale, ToFormattedString};
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

    info!("Job started");

    // fetch all block‐hashes
    let t0 = Instant::now();
    let tip = rpc.get_block_count()? as usize;
    let mut block_hashes = Vec::with_capacity(tip + 1);
    for h in 0..=tip {
        block_hashes.push(rpc.get_block_hash(h as u64)?);
    }
    info!(
        "fetched {} block‐hashes in {}",
        block_hashes.len().to_formatted_string(&Locale::en),
        format_duration(t0.elapsed())
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
            "batch {}: {} leaves in {} (offset {})",
            batch_idx.to_formatted_string(&Locale::en),
            leaves.len().to_formatted_string(&Locale::en),
            format_duration(t1.elapsed()),
            offset.to_formatted_string(&Locale::en)
        );
        offset += batch_size;
        batch_idx += 1;
    }

    info!(
        "Processed {} batches in total for {}",
        batch_idx.to_formatted_string(&Locale::en),
        format_duration(t0.elapsed())
    );

    // write out
    dump_block_hashes(&block_hashes, &out_dir.join("block_hashes.bin"))?;
    dump_mem_forest(&forest, &out_dir.join("mem_forest.bin"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{hashes::Hash, hex::DisplayHex};
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

    #[test]
    fn get_leaf_hashes_matches_manual() {
        // These values come from “extract_from_parquet.sh” or “extract_from_block.sh”
        const TXID_HEX: &str = "4814f3bd6ad0f372be1375a2e501914cbab4d2feaefe1d125d91bc3145202a00";
        const VOUT: u32 = 0;
        const AMOUNT: u64 = 8662; // in sats
        const HEIGHT: u32 = 699777;
        const BLOCK_HASH_HEX: &str =
            "0000000000000000000a8edc1b8a0e5f5a0b8a0e5f5a0b8a0e5f5a0b8a0e5f5a"; // 32‑byte LE hex
        const SCRIPT_HEX: &str = "00140000000000e90455a22f968c30feabd2fb4c4958";
        const EXPECTED_LEAF_HASH: &str =
            "d7565793d4552d28753064a2a0ffbf15f03721e5effb0789ae6f7e409f706276";

        // from parquet‐row path
        let block_hash: BlockHash = BLOCK_HASH_HEX.parse().unwrap();
        let txid: Txid = TXID_HEX.parse().unwrap();
        let script_pubkey = ScriptBuf::from_bytes(hex::decode(SCRIPT_HEX).unwrap());
        let leaf1 = LeafData {
            block_hash,
            prevout: OutPoint { txid, vout: VOUT },
            header_code: HEIGHT << 1,
            utxo: TxOut {
                value: Amount::from_sat(AMOUNT),
                script_pubkey,
            },
        };

        let got1 = leaf1.get_leaf_hashes().to_string();

        assert_eq!(got1, EXPECTED_LEAF_HASH);
    }

    #[test]
    #[ignore = "requires Bitcoin‑Core RPC + correct env vars"]
    fn leaf_hash_rpc_consistency() {
        // constants from your DuckDB row
        const TXID_HEX: &str = "4814f3bd6ad0f372be1375a2e501914cbab4d2feaefe1d125d91bc3145202a00";
        const VOUT: u32 = 0;
        const AMOUNT: u64 = 8662;
        const HEIGHT: u32 = 699777;
        const SCRIPT_HEX: &str = "00140000000000e90455a22f968c30feabd2fb4c4958";

        // Connect to core
        let rpc_url = env::var("BITCOIN_CORE_RPC_URL").unwrap();
        let cookie = env::var("BITCOIN_CORE_COOKIE_FILE").unwrap();
        let rpc = Client::new(&rpc_url, Auth::CookieFile(PathBuf::from(cookie))).unwrap();

        // 1) Parquet‐side LeafData (we only know height, so fetch the true block hash here)
        let block_hash = rpc.get_block_hash(HEIGHT as u64).unwrap();
        let txid: Txid = TXID_HEX.parse().unwrap();
        let script1 = ScriptBuf::from_bytes(hex::decode(SCRIPT_HEX).unwrap());
        let leaf_parquet = LeafData {
            block_hash,
            prevout: OutPoint { txid, vout: VOUT },
            header_code: HEIGHT << 1,
            utxo: TxOut {
                value: Amount::from_sat(AMOUNT),
                script_pubkey: script1.clone(),
            },
        };
        let hash_parquet = leaf_parquet.get_leaf_hashes();

        // 2) RPC‐side LeafData
        // 2a) verify gettxout
        let out = rpc
            .get_tx_out(&txid, VOUT, None)
            .unwrap()
            .expect("UTXO must exist");
        assert_eq!(out.value.to_sat(), AMOUNT);
        assert_eq!(
            out.script_pub_key
                .hex
                .to_hex_string(bitcoin::hex::Case::Lower),
            SCRIPT_HEX
        );

        // 2b) verify raw‐transaction output matches
        let raw_tx = rpc.get_raw_transaction(&txid, Some(&block_hash)).unwrap();
        let rpc_utxo = raw_tx.output[VOUT as usize].clone();
        assert_eq!(rpc_utxo.value.to_sat(), AMOUNT);
        assert_eq!(rpc_utxo.script_pubkey.to_bytes(), script1.to_bytes());

        let leaf_rpc = LeafData {
            block_hash,
            prevout: OutPoint { txid, vout: VOUT },
            header_code: HEIGHT << 1,
            utxo: rpc_utxo,
        };
        let hash_rpc = leaf_rpc.get_leaf_hashes();

        assert_eq!(
            hash_parquet, hash_rpc,
            "Parquet leaf‐hash = {:?}, RPC leaf‐hash = {:?}",
            hash_parquet, hash_rpc
        );
    }
}

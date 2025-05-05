//! Re-hosted helper functions that previously lived in the standalone
//! `script/` crate.  Keeping them here allows us to drop the path
//! dependency and ship a single crate.

use anyhow::{Context, Result};

// -------------------------------------------------------------------
// Parquet → leaf-hash extraction (was script/src/lib.rs)
// -------------------------------------------------------------------

pub mod parquet {
    use super::*;
    use bitcoin::hashes::{sha256d::Hash as Sha256dHash, Hash};
    use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, TxOut};
    use duckdb::Connection;
    use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    use std::path::Path;
    use utreexo::LeafData;

    /// Extract all leaf hashes from every *non-coinbase* UTXO row in a
    /// Parquet export created by Bitcoin Core’s `dumptxoutset`.  This
    /// matches the behaviour of the original script.
    pub fn get_all_leaf_hashes<P: AsRef<Path>>(parquet: P) -> Result<Vec<BitcoinNodeHash>> {
        let parquet = parquet.as_ref();
        let conn = Connection::open_in_memory().context("open in-mem DuckDB")?;
        let path_str = parquet.to_str().context("invalid UTF-8 in Parquet path")?;
        let sql = format!(
            "SELECT txid, amount, vout, height, script FROM '{}' WHERE coinbase = FALSE",
            path_str
        );
        let mut stmt = conn.prepare(&sql).context("prepare DuckDB query")?;
        let mut leaves = Vec::new();
        for row in stmt.query_map([], |r| {
            let txid_hex: String = r.get(0)?;
            let sats: u64 = r.get(1)?;
            let vout: u32 = r.get(2)?;
            let height: u64 = r.get(3)?;
            let script_bytes: Vec<u8> = r.get(4)?;

            let block_hash = BlockHash::from_raw_hash(Sha256dHash::all_zeros());
            let txid = txid_hex.parse().unwrap();
            let prevout = OutPoint { txid, vout };
            let header_code = (height as u32) << 1;
            let utxo = TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: ScriptBuf::from_bytes(script_bytes),
            };
            let leaf = LeafData {
                block_hash,
                prevout,
                header_code,
                utxo,
            };
            Ok(leaf.get_leaf_hashes())
        })? {
            leaves.push(row?);
        }
        Ok(leaves)
    }
}

// -------------------------------------------------------------------
// Bitcoin RPC abstraction & helper (was script/src/updater.rs)
// -------------------------------------------------------------------

pub mod btc_rpc {
    use super::*;
    use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, TxOut};
    use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    use utreexo::LeafData;

    pub trait BitcoinRpc {
        fn get_block_hash(&self, height: u64) -> Result<BlockHash>;
        fn get_block(&self, hash: &BlockHash) -> Result<bitcoin::Block>;
        fn get_txout(&self, prevout: &OutPoint) -> Result<(u64, Vec<u8>)>;
        fn get_block_height(&self, hash: &BlockHash) -> Result<u32>;
    }

    /// Fetch all non-coinbase inputs of a block as leaf hashes.
    pub fn get_block_leaf_hashes<R: BitcoinRpc>(
        rpc: &R,
        height: u64,
    ) -> Result<Vec<BitcoinNodeHash>> {
        let block_hash = rpc.get_block_hash(height)?;
        let block = rpc.get_block(&block_hash)?;
        let hdr_height = rpc.get_block_height(&block_hash)?;

        let mut hashes = Vec::new();
        for tx in block.txdata.iter() {
            if tx.is_coinbase() {
                continue;
            }
            for txin in &tx.input {
                let prev = &txin.previous_output;
                let (value, script_bytes) = rpc.get_txout(prev)?;
                let utxo = TxOut {
                    value: Amount::from_sat(value),
                    script_pubkey: ScriptBuf::from_bytes(script_bytes),
                };
                let header_code = hdr_height << 1;
                let leaf = LeafData {
                    block_hash,
                    prevout: *prev,
                    header_code,
                    utxo,
                };
                hashes.push(leaf.get_leaf_hashes());
            }
        }
        Ok(hashes)
    }
}

// -------------------------------------------------------------------
// MemForest → Pollard conversion helper (was script/src/pollard.rs)
// -------------------------------------------------------------------

pub mod pollard_conv {
    use super::*;
    use rustreexo::accumulator::mem_forest::MemForest;
    use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    use rustreexo::accumulator::pollard::Pollard;
    use std::io::Cursor;

    pub fn forest_to_pollard(
        bytes: &[u8],
        deletes: &[BitcoinNodeHash],
    ) -> Result<Pollard<BitcoinNodeHash>> {
        let mut cursor = Cursor::new(bytes);
        let mem = MemForest::<BitcoinNodeHash>::deserialize(&mut cursor)
            .context("deserialize MemForest")?;
        let proof = mem
            .prove(deletes)
            .map_err(|e| anyhow::anyhow!("prove: {e:?}"))?;
        let remember = proof.targets.clone();
        let roots = mem
            .get_roots()
            .iter()
            .map(|r| r.get_data())
            .collect::<Vec<_>>();
        let mut pollard = Pollard::from_roots(roots, mem.leaves);
        pollard
            .ingest_proof(proof, deletes, &remember)
            .map_err(|e| anyhow::anyhow!("ingest: {e:?}"))?;
        Ok(pollard)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use rustreexo::accumulator::mem_forest::MemForest;
        use rustreexo::accumulator::node_hash::BitcoinNodeHash;

        #[test]
        fn invalid_bytes_should_error() {
            let bad = vec![0u8; 8];
            let res = forest_to_pollard(&bad, &[]);
            assert!(res.is_err());
        }

        #[test]
        fn happy_path_roots_and_removals() {
            // Build a small forest
            let leaves: Vec<BitcoinNodeHash> = (0..4)
                .map(|i| BitcoinNodeHash::new([i as u8; 32]))
                .collect();
            let mut forest = MemForest::<BitcoinNodeHash>::new();
            forest.modify(&leaves, &[]).unwrap();
            // Serialize it
            let mut buf = Vec::new();
            forest.serialize(&mut buf).unwrap();
            // Remove two leaves
            let deletes = vec![leaves[1], leaves[3]];
            let pollard = forest_to_pollard(&buf, &deletes).expect("should succeed");
            // Pollard roots match forest roots
            let orig_roots: Vec<_> = forest.get_roots().iter().map(|r| r.get_data()).collect();
            let new_roots = pollard.roots();
            assert_eq!(orig_roots, new_roots);
        }
    }
}
// -------------------------------------------------------------------
// Parquet extraction tests
// -------------------------------------------------------------------
#[cfg(test)]
mod parquet_tests {
    use super::parquet::get_all_leaf_hashes;
    use duckdb::Connection;
    use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    use tempfile::tempdir;

    #[test]
    fn test_get_all_leaf_hashes_filters_coinbase() {
        // Setup a temporary Parquet file
        let dir = tempdir().unwrap();
        let path = dir.path().join("utxos.parquet");
        let conn = Connection::open_in_memory().unwrap();
        // Create table
        conn.execute(
            "CREATE TABLE utxos (txid VARCHAR, amount BIGINT, vout INTEGER, height BIGINT, script BLOB, coinbase BOOLEAN)",
            [],
        ).unwrap();
        // Insert rows: one coinbase, two normal
        // 64-char fake txids (32 bytes) – just repeating the character to satisfy the
        // `Txid::from_str` length requirement.  Content does not matter for the hash
        // calculation in this unit test.
        let txid_a = "a".repeat(64);
        let txid_b = "b".repeat(64);
        let txid_c = "c".repeat(64);

        conn.execute(
            &format!("INSERT INTO utxos VALUES ('{txid_a}', 50, 0, 0, x'00', TRUE)"),
            [],
        )
        .unwrap();
        conn.execute(
            &format!("INSERT INTO utxos VALUES ('{txid_b}', 100, 1, 1, x'0102', FALSE)"),
            [],
        )
        .unwrap();
        conn.execute(
            &format!("INSERT INTO utxos VALUES ('{txid_c}', 200, 2, 2, x'0304', FALSE)"),
            [],
        )
        .unwrap();
        // Export to Parquet
        let sql = format!(
            "COPY utxos TO '{}' (FORMAT 'parquet')",
            path.to_string_lossy()
        );
        conn.execute(&sql, []).unwrap();
        // Extract leaves
        let leaves: Vec<BitcoinNodeHash> = get_all_leaf_hashes(&path).unwrap();
        // Should only include the two non-coinbase entries
        assert_eq!(leaves.len(), 2);
    }
}

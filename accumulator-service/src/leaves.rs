//! Extract all Utreexo leaf hashes from a Parquet UTXO dump (coinbase=false).
//! Originally from `script/src/lib.rs`.
use anyhow::{Context, Result};
use bitcoin::hashes::{sha256d::Hash as Sha256dHash, Hash};
use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, TxOut};
use duckdb::Connection;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use std::path::Path;
use utreexo::LeafData;

/// Extract all leaf hashes from a Parquet file where coinbase = FALSE.
/// Returns a vector of leaf hashes (one per UTXO).
pub fn get_all_leaf_hashes<P: AsRef<Path>>(parquet: P) -> Result<Vec<BitcoinNodeHash>> {
    let parquet = parquet.as_ref();
    // Open an in-memory DuckDB connection
    let conn = Connection::open_in_memory().context("failed to open in-memory DuckDB")?;
    let path_str = parquet.to_str().context("invalid parquet path")?;
    // Select all non-coinbase rows
    let sql = format!(
        "SELECT txid, amount, vout, height, script FROM '{}' WHERE coinbase = FALSE",
        path_str
    );
    let mut stmt = conn
        .prepare(&sql)
        .with_context(|| format!("failed to prepare SQL: {}", sql))?;
    let mut leaves = Vec::new();
    for row in stmt.query_map([], |r| {
        // Columns: txid TEXT, amount BIGINT, vout INTEGER, height BIGINT, script BLOB
        let txid_hex: String = r.get(0)?;
        let sats: u64 = r.get(1)?;
        let vout: u32 = r.get(2)?;
        let height: u64 = r.get(3)?;
        let script_bytes: Vec<u8> = r.get(4)?;
        // We don't know the real block hash here, so use zeros placeholder
        let block_hash = BlockHash::from_raw_hash(Sha256dHash::all_zeros());
        // parse hex; invalid hex should panic here (test data is always valid)
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
        leaves.push(row.context("failed to map parquet row to leaf")?);
    }
    Ok(leaves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
    use duckdb::Connection;
    use tempfile::tempdir;

    /// Helper to write a small Parquet file with two UTXO rows.
    fn make_parquet(path: &std::path::Path) -> Result<()> {
        let conn = Connection::open_in_memory().context("failed to open in-memory DuckDB")?;
        conn.execute(
            "CREATE TABLE utxos(
                txid TEXT,
                amount BIGINT,
                vout INTEGER,
                height BIGINT,
                script BLOB,
                coinbase BOOLEAN
             )",
            [],
        )
        .context("failed to create utxos table")?;
        conn.execute(
            "INSERT INTO utxos VALUES
               ('0000000000000000000000000000000000000000000000000000000000000000', 100, 0, 1, X'010203', FALSE),
               ('1111111111111111111111111111111111111111111111111111111111111111', 200, 1, 2, X'0A0B0C0D', FALSE)",
            [],
        ).context("failed to insert test rows into utxos")?;
        let pq = path.to_str().unwrap();
        conn.execute(&format!("COPY utxos TO '{}' (FORMAT 'parquet')", pq), [])
            .context("failed to export Parquet file")?;
        Ok(())
    }

    #[test]
    fn test_get_all_leaf_hashes_happy_path() {
        let dir = tempdir().unwrap();
        let pq_path = dir.path().join("test.parquet");
        make_parquet(&pq_path).expect("failed to write parquet");
        let hashes = get_all_leaf_hashes(&pq_path).expect("get_all_leaf_hashes failed");
        assert_eq!(hashes.len(), 2);
        assert_ne!(hashes[0], BitcoinNodeHash::default());
        assert_ne!(hashes[1], BitcoinNodeHash::default());
    }

    #[test]
    fn test_get_all_leaf_hashes_missing_file() {
        assert!(get_all_leaf_hashes("no_such.parquet").is_err());
    }
}

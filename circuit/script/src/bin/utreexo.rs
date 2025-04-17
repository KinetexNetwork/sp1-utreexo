use duckdb::{Connection, Result as DuckResult};
use num_format::{Locale, ToFormattedStr, ToFormattedString};
use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use sha2::{Digest, Sha256};

fn main() -> DuckResult<()> {
    env_logger::init();
    let parquet_file = std::env::args()
        .nth(1)
        .expect("Usage: utreexo <parquet_file>");

    let conn = Connection::open(":memory:")?;
    let batch_size = 10_000_usize;
    let mut offset = 0_usize;
    let mut mem_forest = MemForest::<BitcoinNodeHash>::new();

    loop {
        // SQL: select only non-coinbase rows
        let sql = format!(
            "SELECT txid, amount, vout, height FROM '{}' WHERE coinbase IS NULL OR coinbase = 0 LIMIT {} OFFSET {}",
            parquet_file, batch_size, offset
        );
        let mut stmt = conn.prepare(&sql)?;
        let hashes: Vec<BitcoinNodeHash> = stmt
            .query_map([], |row| {
                let txid: String = row.get(0)?;
                let amount: u64 = row.get(1)?;
                let vout: u64 = row.get(2)?;
                let height: u64 = row.get(3)?;

                // txid is hex string. Convert to bytes.
                let mut hasher = Sha256::new();
                let txid_bytes = hex::decode(&txid).expect("Invalid hex in txid");
                hasher.update(&txid_bytes);
                hasher.update(&amount.to_le_bytes());
                hasher.update(&vout.to_le_bytes());
                hasher.update(&height.to_le_bytes());

                // Use digest as BitcoinNodeHash (32 bytes)
                let hash = hasher.finalize();
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&hash[..32]);
                Ok(BitcoinNodeHash::from(arr))
            })?
            .map(|r| r.unwrap())
            .collect();

        if hashes.is_empty() {
            break;
        }

        mem_forest
            .modify(&hashes, &[])
            .expect("MemForest modify failed");

        offset += batch_size;
        log::info!(
            "Processed {} items so far",
            offset.to_formatted_string(&Locale::en)
        );
    }

    log::info!("Final MemForest has {} leaves.", mem_forest.leaves);
    Ok(())
}

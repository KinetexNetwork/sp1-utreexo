// Example to fetch all UTXO inputs spent in a given block via Bitcoin Core RPC.
//
// Usage:
//   cargo run --example get_block_inputs -- <rpc_url> <cookie_file> <block_height> <output_file>
//
// This example connects to a Bitcoin Core RPC endpoint, retrieves the block by height,
// iterates all non-coinbase transactions, fetches each input's UTXO via RPC,
// computes the corresponding Utreexo leaf hash (using the same LeafData hashing logic),
// and writes each leaf hash (32-byte hex) to the output file, one per line.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use bitcoincore_rpc::{Auth, Client, RpcApi};
// Additional imports for Utreexo leaf hashing
// For serializing TxOut to bytes
use bitcoin::consensus::encode::serialize;
use bitcoin::ScriptBuf;
use bitcoin::TxOut;
// For converting u64 satoshis into Amount (bitcoin v0.32)
use bitcoin::Amount;
// For hex decoding of RPC-provided hex strings
use bitcoin_hashes::hex::FromHex;
// Use sha2 crate for Utreexo leaf hashing
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use sha2::{Digest, Sha512_256};

/// Domain separation tag for Utreexo V1, matching ``common_files/btc_structs.rs``
const UTREEXO_TAG_V1: [u8; 64] = [
    0x5b, 0x83, 0x2d, 0xb8, 0xca, 0x26, 0xc2, 0x5b, 0xe1, 0xc5, 0x42, 0xd6, 0xcc, 0xed, 0xdd, 0xa8,
    0xc1, 0x45, 0x61, 0x5c, 0xff, 0x5c, 0x35, 0x72, 0x7f, 0xb3, 0x46, 0x26, 0x10, 0x80, 0x7e, 0x20,
    0xae, 0x53, 0x4d, 0xc3, 0xf6, 0x42, 0x99, 0x19, 0x99, 0x31, 0x77, 0x2e, 0x03, 0x78, 0x7d, 0x18,
    0x15, 0x6e, 0xb3, 0x15, 0x1e, 0x0e, 0xd1, 0xb3, 0x09, 0x8b, 0xdc, 0x84, 0x45, 0x86, 0x18, 0x85,
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line arguments
    let mut args = env::args().skip(1);
    let rpc_url = args
        .next()
        .expect("Usage: get_block_inputs <rpc_url> <cookie_file> <block_height> <output_file>");
    let cookie_file = args.next().expect("Missing cookie_file");
    let block_height = args.next().expect("Missing block_height").parse::<u64>()?;
    let output_path = args.next().expect("Missing output_file");

    // Connect to Bitcoin Core RPC
    let rpc = Client::new(&rpc_url, Auth::CookieFile(PathBuf::from(cookie_file)))?;

    // Get block hash and block
    let block_hash = rpc.get_block_hash(block_height)?;
    let block = rpc.get_block(&block_hash)?;

    // Prepare output file
    let file = File::create(output_path)?;
    let mut writer = BufWriter::new(file);

    // Iterate transactions and their inputs
    for tx in block.txdata {
        // Skip coinbase transaction
        if tx.is_coinbase() {
            continue;
        }
        for txin in tx.input {
            let prev = txin.previous_output;
            // Fetch UTXO via RPC get_tx_out
            let utxo = rpc
                .get_tx_out(&prev.txid, prev.vout, None)?
                .ok_or(format!("UTXO not found: {}:{}", prev.txid, prev.vout))?;
            // Amount in satoshis
            let value_sats = utxo.value.to_sat();
            // Extract raw scriptPubKey bytes
            let script_bytes = utxo.script_pub_key.hex.clone();
            let script = ScriptBuf::from(script_bytes);
            let txout = TxOut {
                value: Amount::from_sat(value_sats),
                script_pubkey: script,
            };
            // Determine creation block and header code
            let bestblock = utxo.bestblock;
            let confirms = u64::from(utxo.confirmations);
            let header_info = rpc.get_block_header_info(&bestblock)?;
            let blk_height = header_info.height as u64;
            let creation_height = blk_height - confirms + 1;
            let header_code = if utxo.coinbase {
                (creation_height << 1) | 1
            } else {
                creation_height << 1
            };
            // Serialize UTXO
            let ser_utxo = serialize(&txout);
            // Compute leaf hash
            let mut hasher = Sha512_256::new();
            hasher.update(&UTREEXO_TAG_V1);
            hasher.update(&UTREEXO_TAG_V1);
            let bh_bytes: Vec<u8> = FromHex::from_hex(&bestblock.to_string())?;
            hasher.update(&bh_bytes);
            let txid_bytes: Vec<u8> = FromHex::from_hex(&prev.txid.to_string())?;
            hasher.update(&txid_bytes);
            hasher.update(&prev.vout.to_le_bytes());
            hasher.update(&header_code.to_le_bytes());
            hasher.update(&ser_utxo);
            let leaf_bytes = hasher.finalize();
            let node_hash = BitcoinNodeHash::from(leaf_bytes.as_slice());
            writeln!(writer, "{}", node_hash)?;
        }
    }

    Ok(())
}

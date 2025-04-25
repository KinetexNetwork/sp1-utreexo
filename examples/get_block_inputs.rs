// Example to fetch all UTXO inputs spent in a given block via Bitcoin Core RPC.
//
// Usage:
//   cargo run --example get_block_inputs -- <rpc_url> <cookie_file> <block_height> <output_file>
//
// This example connects to a Bitcoin Core RPC endpoint, retrieves the block by height,
// iterates all non-coinbase transactions, fetches each input’s UTXO via RPC,
// computes the corresponding Utreexo leaf hash, and writes each leaf hash (32-byte hex)
// to the output file, one per line.

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use bitcoin::Amount;
use bitcoin::OutPoint;
use bitcoin::ScriptBuf;
use bitcoin::TxOut;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use utreexo::LeafData;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let rpc_url = args
        .next()
        .expect("Usage: get_block_inputs <rpc_url> <cookie_file> <block_height> <output_file>");
    let cookie_file = args.next().expect("Missing cookie_file");
    let block_height = args.next().expect("Missing block_height").parse::<u64>()?;
    let mut output_path = args.next().expect("Missing output_file");
    if output_path == "-o" || output_path == "--output" {
        output_path = args.next().expect("Missing output_file after flag");
    }

    let rpc = Client::new(&rpc_url, Auth::CookieFile(PathBuf::from(&cookie_file)))?;
    let block_hash = rpc.get_block_hash(block_height)?;
    let block = rpc.get_block(&block_hash)?;

    // Sequentially fetch inputs and compute leaf hashes
    let file = File::create(output_path)?;
    let mut writer = BufWriter::new(file);
    for tx in block.txdata {
        if tx.is_coinbase() {
            continue;
        }
        for txin in tx.input {
            let prev = txin.previous_output;
            let prev_tx = rpc.get_raw_transaction_info(&prev.txid, None)?;
            let vout_info = prev_tx
                .vout
                .iter()
                .find(|v| v.n == prev.vout)
                .cloned()
                .ok_or(format!("vout {} not found in {}", prev.vout, prev.txid))?;
            let value_sats = vout_info.value.to_sat();
            let script = ScriptBuf::from(vout_info.script_pub_key.hex.clone());
            let txout = TxOut {
                value: Amount::from_sat(value_sats),
                script_pubkey: script,
            };
            let bestblock = prev_tx
                .blockhash
                .ok_or(format!("no blockhash for tx {}", prev.txid))?;
            let header_info = rpc.get_block_header_info(&bestblock)?;
            let creation_height = header_info.height as u32;
            let header_code = if prev_tx.is_coinbase() {
                (creation_height << 1) | 1
            } else {
                creation_height << 1
            };
            let leaf = LeafData {
                block_hash: bestblock,
                prevout: OutPoint { txid: prev.txid, vout: prev.vout },
                header_code,
                utxo: txout,
            };
            let node_hash = leaf.get_leaf_hashes();
            writeln!(writer, "{}", node_hash)?;
        }
    }

    Ok(())
}

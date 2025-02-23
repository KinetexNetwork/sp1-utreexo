use bitcoin::consensus::Encodable;
use bitcoin::{Block, OutPoint, Transaction, TxIn, Txid};
use bitcoin_hashes::Hash;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::btc_structs::{BatchProof, LeafData};

fn compute_txid(tx: &Transaction) -> Txid {
    let mut tx_bytes = Vec::new();

    tx.version
        .consensus_encode(&mut tx_bytes)
        .expect("engines don't error");
    tx.input
        .consensus_encode(&mut tx_bytes)
        .expect("engines don't error");
    tx.output
        .consensus_encode(&mut tx_bytes)
        .expect("engines don't error");
    tx.lock_time
        .consensus_encode(&mut tx_bytes)
        .expect("engines don't error");

    let hash = Sha256::digest(&tx_bytes);
    let hash = Sha256::digest(&hash);
    let hash_bytes = hash.as_slice();
    Txid::from_slice(hash_bytes).unwrap()
}

pub fn process_block(
    block: &Block,
    height: u32,
    acc: &mut Pollard,
    input_leaf_hashes: HashMap<TxIn, BitcoinNodeHash>,
) -> BatchProof {
    let mut inputs = Vec::new();
    let mut utxos = Vec::new();
    for tx in block.txdata.iter() {
        let txid = compute_txid(tx);
        for input in tx.input.iter() {
            if !tx.is_coinbase() {
                let hash = *input_leaf_hashes.get(input).unwrap();
                if let Some(idx) = utxos.iter().position(|h| *h == hash) {
                    utxos.remove(idx);
                } else {
                    inputs.push(hash);
                }
            }
        }
        let block_hash = block.block_hash();
        for (idx, output) in tx.output.iter().enumerate() {
            // TODO: doublecheck if is_op_return is proper method
            if !output.script_pubkey.is_op_return() {
                let header_code = if tx.is_coinbase() {
                    height << 1 | 1
                } else {
                    height << 1
                };
                let leaf = LeafData {
                    block_hash,
                    header_code,
                    prevout: OutPoint {
                        txid,
                        vout: idx as u32,
                    },
                    utxo: output.to_owned(),
                };
                utxos.push(leaf.get_leaf_hashes());
            }
        }
    }
    acc.modify(&utxos, &inputs).unwrap();
    BatchProof {
        targets: vec![],
        hashes: vec![],
    }
}

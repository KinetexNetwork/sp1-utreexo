use bitcoin::consensus::Encodable;
use bitcoin::{Block, BlockHash, OutPoint, TxIn, VarInt};
use bitcoin_hashes::sha256d::Hash;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;

use crate::btc_structs::{BatchProof, LeafData};

pub fn process_block(
    block: &Block,
    height: u32,
    acc: &mut Pollard,
    input_leaf_hashes: HashMap<TxIn, BitcoinNodeHash>,
) -> BatchProof {
    let mut inputs = Vec::new();
    let mut utxos = Vec::new();
    for tx in block.txdata.iter() {
        let txid = tx.compute_txid();
        for input in tx.input.iter() {
            if !tx.is_coinbase() {
                let hash = input_leaf_hashes.get(&input).unwrap().clone();
                if let Some(idx) = utxos.iter().position(|h| *h == hash) {
                    utxos.remove(idx);
                } else {
                    inputs.push(hash);
                }
            }
        }
        for (idx, output) in tx.output.iter().enumerate() {
            // TODO: doublecheck if is_op_return is proper method
            if !output.script_pubkey.is_op_return() {
                let header_code = if tx.is_coinbase() {
                    height << 1 | 1
                } else {
                    height << 1
                };
                let leaf = LeafData {
                    block_hash: block.block_hash(),
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

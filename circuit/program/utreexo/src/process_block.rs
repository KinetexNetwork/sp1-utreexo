use bitcoin::consensus::Encodable;
use bitcoin::{Block, BlockHash, OutPoint, TxIn, VarInt};
use bitcoin_hashes::sha256d::Hash;
use rustreexo::accumulator::node_hash::NodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;

use crate::btc_structs::{BatchProof, CompactLeafData, LeafData};

pub fn process_block(
    block: &Block,
    height: u32,
    acc: &mut Pollard,
    input_leaf_hashes: HashMap<TxIn, (NodeHash, CompactLeafData)>,
) -> (BatchProof, Vec<CompactLeafData>) {
    let mut inputs = Vec::new();
    let mut utxos = Vec::new();
    let mut compact_leaves = Vec::new();
    for tx in block.txdata.iter() {
        let txid = tx.compute_txid();
        for input in tx.input.iter() {
            if !tx.is_coinbase() {
                let (hash, compact_leaf) = input_leaf_hashes.get(&input).unwrap().clone();
                if let Some(idx) = utxos.iter().position(|h| *h == hash) {
                    utxos.remove(idx);
                } else {
                    inputs.push(hash);
                    compact_leaves.push(compact_leaf);
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
    let proof = acc.prove(&inputs).unwrap();
    acc.modify(&utxos, &inputs).unwrap();
    let mut ser_acc = Vec::new();
    acc.leaves.consensus_encode(&mut ser_acc).unwrap();
    acc.get_roots().iter().for_each(|x| {
        x.get_data().consensus_encode(&mut ser_acc).unwrap();
    });
    (
        BatchProof {
            targets: proof.targets.iter().map(|target| VarInt(*target)).collect(),
            hashes: proof
                .hashes
                .iter()
                .map(|hash| BlockHash::from_raw_hash(*Hash::from_bytes_ref(&*hash)))
                .collect(),
        },
        compact_leaves,
    )
}

//SPDX-License-Identifier: MIT
#![no_main]
sp1_zkvm::entrypoint!(main);

use alloy_sol_types::{sol, SolType};
use bitcoin::{Block, TxIn};
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;
use std::ops::Deref;

mod btc_structs;
mod process_block;

use crate::process_block::process_block;

type PublicValuesTuple = sol! {
    (
        bytes, // acc roots
    )
};

pub fn main() {
    let block = sp1_zkvm::io::read::<Block>();
    let height = sp1_zkvm::io::read::<u32>();

    let mut acc = sp1_zkvm::io::read::<Pollard>();
    let input_leaf_hashes = sp1_zkvm::io::read::<HashMap<TxIn, BitcoinNodeHash>>();
    let _proof = process_block(&block, height, &mut acc, input_leaf_hashes);
    let acc_roots: Vec<BitcoinNodeHash> = acc
        .get_roots()
        .to_vec()
        .iter()
        .map(|rc| rc.get_data())
        .collect();
    // TODO: have _proof and _compact_leaves as outputs instead of this
    let acc_roots_bytes: Vec<[u8; 32]> = acc_roots.iter().map(|hash| *hash.deref()).collect();
    let acc_roots_bytes_flat: Vec<u8> = acc_roots_bytes.concat();

    let bytes = PublicValuesTuple::abi_encode(&(acc_roots_bytes_flat,));

    sp1_zkvm::io::commit_slice(&bytes);
}

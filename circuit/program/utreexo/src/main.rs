//SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "native"), no_main)]

#[cfg(not(feature = "native"))]
sp1_zkvm::entrypoint!(main);

use std::collections::HashMap;
use std::ops::Deref;

use alloy_sol_types::sol;
use alloy_sol_types::SolType;
use bitcoin::Block;
use bitcoin::TxIn;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;

mod btc_structs;
mod process_block;

use crate::process_block::process_block;

type PublicValuesTuple = sol! {
    (
        bytes, // acc roots
    )
};

pub fn main() {
    let (block, height, mut acc, input_leaf_hashes) = read_inputs();
    let _proof = process_block(
        &block,
        height,
        &mut acc,
        input_leaf_hashes,
    );
    let acc_roots: Vec<BitcoinNodeHash> = acc
        .get_roots()
        .iter()
        .map(|rc| rc.get_data())
        .collect();
    let acc_roots_bytes: Vec<[u8; 32]> = acc_roots
        .iter()
        .map(|hash| *hash.deref())
        .collect();
    let acc_roots_bytes_flat: Vec<u8> = acc_roots_bytes.concat();

    let bytes = PublicValuesTuple::abi_encode(&(acc_roots_bytes_flat,));
    commit_slice(&bytes);
}

#[cfg(feature = "native")]
fn read_inputs() -> (
    Block,
    u32,
    Pollard,
    HashMap<TxIn, BitcoinNodeHash>,
) {
    use std::io::Read;
    use std::io::{self};

    use atty::Stream;
    use serde_json;

    // If stdin is a tty, then likely no piped input was provided.
    if atty::is(Stream::Stdin) {
        eprintln!("Error: No piped input provided (stdin is a tty).");
        std::process::exit(1);
    }

    let mut input_data = String::new();
    io::stdin()
        .read_to_string(&mut input_data)
        .expect("Failed to read from stdin");

    if input_data.trim().is_empty() {
        eprintln!("Error: Received empty input.");
        std::process::exit(1);
    }

    serde_json::from_str(&input_data)
        .expect("Deserialization failed: Provided input is invalid or cannot be parsed into the required types")
}

#[cfg(not(feature = "native"))]
fn read_inputs() -> (
    Block,
    u32,
    Pollard,
    HashMap<TxIn, BitcoinNodeHash>,
) {
    (
        sp1_zkvm::io::read::<Block>(),
        sp1_zkvm::io::read::<u32>(),
        sp1_zkvm::io::read::<Pollard>(),
        sp1_zkvm::io::read::<HashMap<TxIn, BitcoinNodeHash>>(),
    )
}

#[cfg(feature = "native")]
fn commit_slice(bytes: &[u8]) {
    use std::io::Write;
    use std::io::{self};
    io::stdout()
        .write_all(bytes)
        .unwrap();
}

#[cfg(not(feature = "native"))]
fn commit_slice(bytes: &[u8]) {
    sp1_zkvm::io::commit_slice(bytes);
}

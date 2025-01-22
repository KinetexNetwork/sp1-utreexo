use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Cursor;
use std::io::Write;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;
use std::time::Instant;

use alloy_sol_types::sol;
use alloy_sol_types::SolType;
use bitcoin::consensus::serialize;
use bitcoin::consensus::Encodable;
use bitcoin::Block;
use bitcoin::BlockHash;
use bitcoin::OutPoint;
use bitcoin::Script;
use bitcoin::Transaction;
use bitcoin::TxIn;
use bitcoin::TxOut;
#[cfg(feature = "api")]
use bitcoin::Txid;
use clap::Parser;
#[cfg(feature = "api")]
use futures::channel::mpsc::Receiver;
use log::error;
use log::info;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use rustreexo::accumulator::proof::Proof;
use rustreexo::accumulator::stump::Stump;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_writer_pretty;
use sha2::Digest;
use sha2::Sha512_256;
use sp1_sdk::utils;
use sp1_sdk::ProverClient;
use sp1_sdk::SP1Proof;
use sp1_sdk::SP1ProofWithPublicValues;
use sp1_sdk::SP1ProvingKey;
use sp1_sdk::SP1Stdin;

use crate::block_index::BlockIndex;
use crate::block_index::BlocksIndex;
use crate::chaininterface::Blockchain;
use crate::chainview;
use crate::udata::bitcoin_leaf_data::BitcoinLeafData;
use crate::udata::LeafContext;
use crate::udata::LeafData;
use crate::udata::UtreexoBlock;

type PublicValuesTuple = sol! {
    (
        bytes, // acc roots
    )
};

pub struct ProofStorage {
    proofs: RwLock<HashMap<BlockHash, SP1Proof>>,
}

impl ProofStorage {
    pub fn new() -> Self {
        ProofStorage {
            proofs: RwLock::new(HashMap::new()),
        }
    }
    pub fn add_proof(&self, block_hash: BlockHash, proof: SP1Proof) {
        self.proofs.write().unwrap().insert(block_hash, proof);
    }
    pub fn get_proof(&self, block_hash: &BlockHash) -> Option<SP1Proof> {
        self.proofs.read().unwrap().get(block_hash).cloned()
    }
    pub fn keys(&self) -> Vec<BlockHash> {
        self.proofs.read().unwrap().keys().cloned().collect()
    }
}

pub fn run_circuit(
    block: &Block,
    prepared_acc_before: Pollard,
    input_leaf_hashes: &HashMap<TxIn, BitcoinNodeHash>,
    height: u32,
    prover_client: &ProverClient,
    proving_key: &SP1ProvingKey,
) -> SP1ProofWithPublicValues {
    let mut stdin = SP1Stdin::new();

    stdin.write::<Block>(&block);
    stdin.write::<u32>(&height);
    stdin.write::<Pollard>(&prepared_acc_before);
    stdin.write::<HashMap<TxIn, BitcoinNodeHash>>(&input_leaf_hashes);

    prover_client
        .prove(&proving_key, stdin)
        .run()
        .expect("failed to generate proof")
}

pub fn get_expected_output(acc: &Pollard) -> Vec<u8> {
    let acc_roots: Vec<BitcoinNodeHash> = acc
        .get_roots()
        .to_vec()
        .iter()
        .map(|rc| rc.get_data())
        .collect();
    let acc_roots_bytes: Vec<[u8; 32]> = acc_roots.iter().map(|hash| *hash.deref()).collect();
    let acc_roots_bytes_flat: Vec<u8> = acc_roots_bytes.concat();
    let encoded = PublicValuesTuple::abi_encode(&(acc_roots_bytes_flat,));
    encoded
}

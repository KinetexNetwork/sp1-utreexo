use crate::zk::ProofStorage;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

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
use sp1_sdk::{SP1Proof, SP1VerifyingKey};

use crate::block_index::BlockIndex;
use crate::block_index::BlocksIndex;
use crate::chaininterface::Blockchain;
use crate::chainview;
use crate::udata::LeafContext;
use crate::udata::LeafData;
use crate::udata::UtreexoBlock;


pub struct InMemoryDatabase {
    zk_proof_storage: Arc<ProofStorage>,
    shutdown_flag: Arc<Mutex<bool>>,
    verification_key: SP1VerifyingKey,
}


impl InMemoryDatabase {
    pub fn new(storage: Arc<ProofStorage>, shutdown_flag: Arc<Mutex<bool>>, verification_key: SP1VerifyingKey) -> Self {
        Self {
            zk_proof_storage: storage,
            shutdown_flag,
            verification_key,
        }
    }
    /// A infinite loop that keeps the prover up to date with the blockchain. It handles requests
    /// from other modules and updates the accumulator when a new block is found. This method is
    /// also how we create proofs for historical blocks.
    pub fn keep_up(
        &mut self,
        #[cfg(feature = "api")] mut receiver: Receiver<(
            Requests,
            futures::channel::oneshot::Sender<Result<Responses, String>>,
        )>,
    ) -> anyhow::Result<()> {
        loop {
            if *self.shutdown_flag.lock().unwrap() {
                info!("Shutting down in-memory database");
                break;
            }

            #[cfg(feature = "api")]
            if let Ok(Some((req, res))) = receiver.try_next() {
                let ret = self.handle_request(req).map_err(|e| e.to_string());
                res.send(ret)
                    .map_err(|_| anyhow::anyhow!("Error sending response"))?;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        Ok(())
    }

    #[cfg(feature = "api")]
    fn handle_request(&mut self, req: Requests) -> anyhow::Result<Responses> {
        match req {
            Requests::GetSP1Proof(block_hash) => {
                let proof = self
                    .zk_proof_storage
                    .get_proof(&block_hash)
                    .ok_or(anyhow::anyhow!("Proof not found"))?;
                info!("Prover returned proof: {:#?}", proof);
                Ok(Responses::SP1Proof(proof))
            }
            Requests::GetSP1VerificationKey => Ok(Responses::SP1VerificationKey(
                self.verification_key.clone(),
            )),
        }
    }

    pub fn add_proof(&self, block_hash: BlockHash, proof: SP1Proof) {
        info!("Adding proof for block {} to in-memory database", block_hash);
        self.zk_proof_storage.add_proof(block_hash, proof);
    }

}


#[cfg(feature = "api")]
/// All requests we can send to the prover. The prover will respond with the corresponding
/// response element.
pub enum Requests {
    GetSP1Proof(BlockHash),
    GetSP1VerificationKey,
}
/// All responses the prover will send.
#[derive(Clone, Serialize, Deserialize)]
pub enum Responses {
    SP1Proof(SP1Proof),
    SP1VerificationKey(SP1VerifyingKey),
}

//SPDX-License-Identifier: MIT

//! A prover is a thread that keeps up with the blockchain and generates proofs for
//! the utreexo accumulator. Since it holds the entire accumulator, it also provides
//! proofs for other modules. To avoid having multiple channels to and from the prover, it
//! uses a channel to receive requests and sends responses through a oneshot channel, provided
//! by the request sender. Maybe there is a better way to do this, but this is a TODO for later.
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::io::{self};
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

use crate::block_index::BlockIndex;
use crate::block_index::BlocksIndex;
use crate::chaininterface::Blockchain;
use crate::chainview;
use crate::udata::LeafContext;
use crate::udata::LeafData;
use crate::udata::UtreexoBlock;

#[cfg(not(feature = "shinigami"))]
pub type AccumulatorHash = rustreexo::accumulator::node_hash::BitcoinNodeHash;

pub trait BlockStorage {
    fn save_block(
        &mut self,
        block: &Block,
        block_height: u32,
        proof: Proof,
        leaves: Vec<LeafContext>,
        acc: &Pollard,
    ) -> BlockIndex;
    fn get_block(&self, index: BlockIndex) -> Option<UtreexoBlock>;
}

#[cfg(feature = "shinigami")]
pub type AccumulatorHash = crate::udata::shinigami_udata::PoseidonHash;

pub trait LeafCache: Sync + Send + Sized + 'static {
    fn remove(&mut self, outpoint: &OutPoint) -> Option<LeafContext>;
    fn insert(&mut self, outpoint: OutPoint, leaf_data: LeafContext) -> bool;
    fn flush(&mut self) {}
    fn cache_size(&self) -> usize {
        0
    }
}

impl LeafCache for HashMap<OutPoint, LeafContext> {
    fn remove(&mut self, outpoint: &OutPoint) -> Option<LeafContext> {
        self.remove(outpoint)
    }

    fn insert(&mut self, outpoint: OutPoint, leaf_data: LeafContext) -> bool {
        self.insert(outpoint, leaf_data);
        false
    }
}

/// All the state that the prover needs to keep track of
pub struct Prover<LeafStorage: LeafCache> {
    /// A reference to the RPC client that is used to query the blockchain.
    rpc: Box<dyn Blockchain>,
    /// The accumulator that holds the state of the utreexo accumulator.
    acc: Pollard,
    /// An index that keeps track of the blocks that are stored on disk, we need this
    /// to get the blocks from disk.
    storage: Arc<BlocksIndex>,
    /// The height of the blockchain we are on.
    height: u32,
    /// A reference to the chainview, this keeps a map of block hashes to heights and vice versa.
    /// Also keeps block headers for easy access.
    view: Arc<chainview::ChainView>,
    /// A map that keeps track of the leaf data for each outpoint. This is used to generate
    /// proofs for the utreexo accumulator. This is more like a cache, since it won't be
    /// persisted on shutdown.
    leaf_data: LeafStorage,
    /// If set, we'll save a snapshot of the accumulator to disk every n blocks.
    ///
    /// The file will be named <height>.acc and can be used to start this software from
    /// that height.
    snapshot_acc_every: Option<u32>,
    /// A flag that is set when the prover should shut down.
    shutdown_flag: Arc<Mutex<bool>>,
    /// Only save proofs for blocks older than that
    save_proofs_for_blocks_older_than: u32,
    block_notification: Sender<BlockHash>,
    ibd: bool,
}

#[allow(clippy::too_many_arguments)]
impl<LeafStorage: LeafCache> Prover<LeafStorage> {
    /// Creates a new prover. It loads the accumulator from disk, if it exists.
    pub fn new(
        rpc: Box<dyn Blockchain>,
        index_database: Arc<BlocksIndex>,
        view: Arc<chainview::ChainView>,
        leaf_data: LeafStorage,
        start_acc: Option<PathBuf>,
        start_height: Option<u32>,
        snapshot_acc_every: Option<u32>,
        shutdown_flag: Arc<Mutex<bool>>,
        save_proofs_for_blocks_older_than: u32,
        block_notification: Sender<BlockHash>,
    ) -> Prover<LeafStorage> {
        // TODO: make this dump path configurable
        let (acc, height) = load_acc_from_utxo_dump("./utxodump.csv", &rpc);

        Self {
            snapshot_acc_every,
            rpc,
            acc,
            height,
            storage: index_database,
            view,
            leaf_data,
            shutdown_flag,
            save_proofs_for_blocks_older_than,
            block_notification,
            ibd: true,
        }
    }

    /// Tries to load the accumulator from disk. If it fails, it creates a new one.
    fn try_from_disk(path: Option<PathBuf>) -> Pollard {
        if let Some(path) = path {
            let file = std::fs::File::open(&path).unwrap();
            let reader = std::io::BufReader::new(file);
            match Pollard::deserialize(reader) {
                Ok(acc) => return acc,
                Err(e) => panic!("Failed to load accumulator at {path:?}, reson: {e:?}"),
            }
        }

        let Ok(file) = std::fs::File::open(crate::subdir("/pollard")) else {
            return Pollard::new();
        };

        let reader = std::io::BufReader::new(file);
        match Pollard::deserialize(reader) {
            Ok(acc) => acc,
            Err(_) => Pollard::new(),
        }
    }

    /// Handles the request from another module. It returns a response through the oneshot channel
    /// provided by the request sender. Errors are returned as strings, maybe this should be changed
    /// to a boxed error or something else.
    #[cfg(feature = "api")]
    fn handle_request(&mut self, req: Requests) -> anyhow::Result<Responses> {
        use bitcoin::ScriptBuf;
        use bitcoin::Sequence;
        use bitcoin::Witness;

        match req {
            // Requests::GetSP1Proof(block_hash) => {
            //     let proof = self
            //         .zk_proof_storage
            //         .get_proof(&block_hash)
            //         .ok_or(anyhow::anyhow!("Proof not found"))?;
            //     info!("Prover returned proof: {:#?}", proof);
            //     Ok(Responses::SP1Proof(proof))
            // }
            Requests::GetProof(node) => {
                let proof = self
                    .acc
                    .prove(&[node])
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                Ok(Responses::Proof(proof))
            }
            Requests::GetRoots => {
                let roots = self.acc.get_roots().iter().map(|x| x.get_data()).collect();
                Ok(Responses::Roots(roots))
            }
            Requests::GetBlockByHeight(height) => {
                unimplemented!()
            }
            Requests::GetTxUnpent(txid) => {
                // returns the unspent outputs of a transaction and a proof for them
                let tx = self
                    .rpc
                    .get_transaction(txid)
                    .map_err(|_| anyhow::anyhow!("Transaction {} not found", txid))?;

                let (outputs, hashes): (Vec<TxOut>, Vec<AccumulatorHash>) = tx
                    .output
                    .iter()
                    .enumerate()
                    .flat_map(|(idx, output)| {
                        let (hash, _) = self.get_input_leaf_hash(&TxIn {
                            previous_output: OutPoint {
                                txid,
                                vout: idx as u32,
                            },
                            script_sig: ScriptBuf::new(),
                            sequence: Sequence::ZERO,
                            witness: Witness::new(),
                        });
                        self.acc.prove(&[hash]).ok()?;
                        Some((output.clone(), hash))
                    })
                    .unzip();

                let proof = self
                    .acc
                    .prove(&hashes)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                Ok(Responses::TransactionOut(outputs, proof))
            }
            Requests::GetTransaction(txid) => {
                let tx = self
                    .rpc
                    .get_transaction(txid)
                    .map_err(|_| anyhow::anyhow!("Transaction {} not found", txid))?;
                // TODO: this is a bit of a hack, but it works for now.
                // Rustreexo should have a way to check whether an element is in the
                // pollard. We have this information in the map anyway.
                let (_outputs, hashes): (Vec<TxOut>, Vec<AccumulatorHash>) = tx
                    .output
                    .iter()
                    .enumerate()
                    .flat_map(|(idx, output)| {
                        let (hash, _) = self.get_input_leaf_hash(&TxIn {
                            previous_output: OutPoint {
                                txid,
                                vout: idx as u32,
                            },
                            script_sig: ScriptBuf::new(),
                            sequence: Sequence::ZERO,
                            witness: Witness::new(),
                        });
                        self.acc.prove(&[hash]).ok()?;
                        Some((output.clone(), hash))
                    })
                    .unzip();

                let proof = self
                    .acc
                    .prove(&hashes)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                Ok(Responses::Transaction((tx, proof)))
            }
            Requests::GetCSN => {
                let roots = self.acc.get_roots().iter().map(|x| x.get_data()).collect();
                let leaves = self.acc.leaves;
                Ok(Responses::CSN(Stump { roots, leaves }))
            }
            Requests::GetBlocksByHeight(height, count) => {
                unimplemented!();
            }
            _ => Err(anyhow::anyhow!("Uniplemented request in prover")),
        }
    }

    /// Gracefully shuts down the prover. It saves the accumulator to disk and flushes the chainview.
    fn shutdown(&mut self) {
        self.save_to_disk(None)
            .expect("could not save the acc to disk");
        self.leaf_data.flush();
        self.view.flush();
    }

    /// Saves the accumulator to disk. This is done by serializing the accumulator to a file,
    /// the serialization is done by the rustreexo library and is a depth first traversal of the
    /// tree.
    fn save_to_disk(&self, height: Option<u32>) -> std::io::Result<()> {
        let file = match height {
            Some(height) => std::fs::File::create(crate::subdir(&format!("{}.acc", height)))?,
            None => std::fs::File::create(crate::subdir("/pollard"))?,
        };

        let mut writer = std::io::BufWriter::new(file);
        self.acc.serialize(&mut writer).unwrap();

        Ok(())
    }

    /// A infinite loop that keeps the prover up to date with the blockchain. It handles requests
    /// from other modules and updates the accumulator when a new block is found. This method is
    /// also how we create proofs for historical blocks.
    pub fn keep_up(&mut self) -> anyhow::Result<()> {
        let mut last_tip_update = std::time::Instant::now();
        loop {
            if *self.shutdown_flag.lock().unwrap() {
                info!("Shutting down prover");
                self.shutdown();
                break;
            }

            if last_tip_update.elapsed().as_secs() > 10 {
                if let Err(e) = self.check_tip(&mut last_tip_update) {
                    error!("Error checking tip: {}", e);
                    continue;
                }
            }

            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        self.save_to_disk(None)
            .expect("could not save the acc to disk");
        self.storage.update_height(self.height as usize);
        Ok(())
    }

    fn check_tip(&mut self, last_tip_update: &mut std::time::Instant) -> anyhow::Result<()> {
        let height = self.rpc.get_block_count()? as u32;
        if height > self.height {
            self.prove_range(self.height + 1, height)?;

            self.save_to_disk(None)
                .expect("could not save the acc to disk");
            self.storage.update_height(height as usize);
        }
        self.ibd = false; // we'll flip it once, and keep it false for the rest of the time
        *last_tip_update = std::time::Instant::now();
        Ok(())
    }

    /// Proves a range of blocks, may be just one block.
    pub fn prove_range(&mut self, start: u32, end: u32) -> anyhow::Result<()> {
        for height in start..=end {
            if *self.shutdown_flag.lock().unwrap() {
                break;
            }

            let block_hash = self.rpc.get_block_hash(height as u64)?;
            // Update the local index
            self.view.save_block_hash(height, block_hash)?;
            self.view.save_height(block_hash, height)?;

            let block = self.rpc.get_block(block_hash)?;

            self.view
                .save_header(block_hash, serialize(&block.header))?;

            info!(
                "processing height={} cache={} txs={}",
                height,
                self.leaf_data.cache_size(),
                block.txdata.len()
            );

            let mtp = self.rpc.get_mtp(block.header.prev_blockhash)?;

            let (proof, leaves) = self.process_block(&block, height, mtp);

            self.height = height;
            if let Some(n) = self.snapshot_acc_every {
                info!("doing backup");
                if height % n == 0 {
                    self.save_to_disk(Some(height))
                        .expect("could not save the acc to disk");
                }
            }

            if !self.ibd {
                // only notify when we're not in IBD
                self.block_notification.send(block.block_hash()).unwrap();
            }
        }

        anyhow::Ok(())
    }

    /// Pulls the [LeafData] from the bitcoin core rpc. We use this as fallback if we can't find
    /// the leaf in leaf_data. This method is slow and should only be used if we can't find the
    /// leaf in the leaf_data.
    fn get_input_leaf_hash_from_rpc(rpc: &dyn Blockchain, input: &TxIn) -> Option<LeafContext> {
        let tx_info = rpc
            .get_raw_transaction_info(&input.previous_output.txid)
            .ok()?;

        let height = tx_info.height;
        let output = &tx_info.tx.output[input.previous_output.vout as usize];
        let prev_block = rpc
            .get_block_header(tx_info.blockhash?)
            .ok()?
            .prev_blockhash;

        let median_time_past = rpc.get_mtp(prev_block).ok()?;

        Some(LeafContext {
            block_hash: tx_info.blockhash?,
            median_time_past,
            block_height: height,
            is_coinbase: tx_info.is_coinbase,
            pk_script: output.script_pubkey.clone(),
            value: output.value.to_sat(),
            vout: input.previous_output.vout,
            txid: input.previous_output.txid,
        })
    }

    /// Returns the leaf hash and the compact leaf data for a given input. If the leaf is not in
    /// leaf_data we will try to get it from the bitcoin core rpc.
    fn get_input_leaf_hash(&mut self, input: &TxIn) -> (AccumulatorHash, LeafContext) {
        let leaf = self
            .leaf_data
            .remove(&input.previous_output)
            .unwrap_or_else(|| Self::get_input_leaf_hash_from_rpc(&*self.rpc, input).unwrap());

        (LeafData::get_leaf_hashes(&leaf), leaf)
    }

    fn is_unspendable(script: &Script) -> bool {
        if script.len() > 10_000 {
            return true;
        }

        if !script.is_empty() && script.as_bytes()[0] == 0x6a {
            return true;
        }

        false
    }

    /// Processes a block and returns the batch proof and the compact leaf data for the block.
    fn process_block(&mut self, block: &Block, height: u32, mtp: u32) -> (Proof, Vec<LeafContext>) {
        let mut inputs = Vec::new();
        let mut utxos = Vec::new();
        let mut compact_leaves = Vec::new();

        let mut input_leaf_hashes: HashMap<TxIn, BitcoinNodeHash> = Default::default();

        for tx in block.txdata.iter() {
            let txid = tx.compute_txid();
            for input in tx.input.iter() {
                if !tx.is_coinbase() {
                    let (hash, compact_leaf) = self.get_input_leaf_hash(input);
                    input_leaf_hashes.insert(input.clone(), hash);
                    if let Some(idx) = utxos.iter().position(|h| *h == hash) {
                        utxos.remove(idx);
                    } else {
                        inputs.push(hash);
                        compact_leaves.push(compact_leaf);
                    }
                }
            }

            for (idx, output) in tx.output.iter().enumerate() {
                if !Self::is_unspendable(&output.script_pubkey) {
                    let leaf = LeafContext {
                        block_hash: block.block_hash(),
                        median_time_past: mtp,
                        txid,
                        vout: idx as u32,
                        value: output.value.to_sat(),
                        pk_script: output.script_pubkey.clone(),
                        is_coinbase: tx.is_coinbase(),
                        block_height: height,
                    };

                    utxos.push(LeafData::get_leaf_hashes(&leaf));

                    let flush = self.leaf_data.insert(
                        OutPoint {
                            txid,
                            vout: idx as u32,
                        },
                        leaf,
                    );

                    if flush {
                        info!("Flushing leaf data, height={}", height);
                        self.leaf_data.flush();
                        self.save_to_disk(None)
                            .expect("could not save the acc to disk");
                        self.storage.update_height(self.height as usize);
                    }
                }
            }
        }

        let proof = self.acc.prove(&inputs).unwrap();

        // if !self.zk_proof_storage.keys().contains(&block.block_hash()) {
        //     // do some zk stuff
        //     let flagged_pollard = self.acc.clone().fake_modify(&utxos, &inputs);
        //     let stripped_pollard = flagged_pollard.get_stripped_pollard();
        //     let sp1_proof = zk::run_circuit(block, stripped_pollard, &input_leaf_hashes, height, &self.prover_client, &self.proving_key);
        //     let public_values = sp1_proof.public_values.as_slice();
        //     self.acc.modify(&utxos, &inputs).unwrap();
        //     let expected_public_values = zk::get_expected_output(&self.acc);
        //     assert_eq!(public_values, expected_public_values);
        //     self.zk_proof_storage
        //         .add_proof(block.block_hash(), sp1_proof.proof);
        // } else {
        //     self.acc.modify(&utxos, &inputs).unwrap();
        // }
        self.acc.modify(&utxos, &inputs).unwrap(); // rm this when uncomment above

        (proof, compact_leaves)
    }
}

#[cfg(feature = "api")]
/// All requests we can send to the prover. The prover will respond with the corresponding
/// response element.
pub enum Requests {
    /// Get the proof for a given leaf hash.
    GetProof(BitcoinNodeHash),
    /// Get the roots of the accumulator.
    GetRoots,
    /// Get a block at a given height. This method returns the block and utreexo data for it.
    GetBlockByHeight(u32),
    /// Returns a transaction and a proof for all inputs
    GetTransaction(Txid),
    /// Returns the CSN of the current acc
    GetCSN,
    /// Returns multiple blocks and utreexo data for them.
    GetBlocksByHeight(u32, u32),
    GetTxUnpent(Txid),
    // Returns SP1 proof corresponding to utreexo mutation during this block
    GetSP1Proof(BlockHash),
}
/// All responses the prover will send.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Responses {
    /// A utreexo proof
    Proof(Proof),
    /// The roots of the accumulator
    Roots(Vec<BitcoinNodeHash>),
    /// A block and the utreexo data for it, serialized.
    Block(Vec<u8>),
    /// A transaction and a proof for all **outputs**
    Transaction((Transaction, Proof)),
    /// The CSN of the current acc
    #[allow(clippy::upper_case_acronyms)]
    CSN(Stump),
    /// Multiple blocks and utreexo data for them.
    Blocks(Vec<Vec<u8>>),
    TransactionOut(Vec<TxOut>, Proof),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CsvUtxo {
    count: u64,
    txid: String,
    vout: u32,
    pub height: u32,
    coinbase: u8,
    amount: u64,
    script: String,
    #[serde(rename = "type")]
    tx_type: String,
    address: String,
}
use crate::udata::bitcoin_leaf_data::BitcoinLeafData;
use std::str::FromStr;
use bitcoin::Amount;
use bitcoin::ScriptBuf;
impl CsvUtxo {
    pub fn as_bitcoin_leaf_data(self, rpc: &Box<dyn Blockchain>) -> BitcoinLeafData {
        let prevout = OutPoint {
            txid: Txid::from_str(&self.txid).unwrap(),
            vout: self.vout,
        };
        let header_code = if self.coinbase == 1 {
            self.height << 1 | 1
        } else {
            self.height << 1
        };
        let utxo = TxOut {
            value: Amount::from_sat(self.amount),
            script_pubkey: ScriptBuf::from_hex(&self.script).unwrap(),
        };
        let block_hash = rpc.get_block_hash(self.height as u64).unwrap();
        BitcoinLeafData {
            block_hash,
            prevout,
            header_code,
            utxo,
        }
    }
}

/// Loads the accumulator from a utxo dump. Returns loaded pollard and the block this Pollard corresponds to
fn load_acc_from_utxo_dump(
    utxo_dump_path: &str,
    rpc: &Box<dyn Blockchain>,
)  -> (Pollard, u32) {
    let file = File::open(utxo_dump_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut leaf_datas = Vec::new();
    let mut max_height = 0;
    for (idx, result) in rdr.deserialize().enumerate() {
        if idx % 10000 == 0 {
            info!("Loaded utxos: {}", idx);
        }
        let utxo: CsvUtxo = result.unwrap();
        
        if utxo.height > max_height {
            max_height = utxo.height;
        }

        let leaf_data = utxo.as_bitcoin_leaf_data(rpc);
        leaf_datas.push(leaf_data);
    }
    let leaf_hashes = leaf_datas
        .iter()
        .map(|leaf_data|leaf_data.compute_hash())
        .collect::<Vec<_>>();
    let mut acc = Pollard::new();
    acc.modify(&leaf_hashes, &[]).unwrap();
    (acc, max_height)
}

use bitcoin::consensus::Encodable;
use bitcoin::Block;
use bitcoin::TxIn;
use bitcoin::{BlockHash, OutPoint, TxOut, VarInt};
use clap::Parser;
use regex::Regex;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use serde::{Deserialize, Serialize};
use serde_json::to_writer_pretty;
use sha2::{Digest, Sha512_256};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Cursor;

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum ScriptPubkeyType {
    /// An non-specified type, in this case the script is just copied over
    Other(Box<[u8]>),
    /// p2pkh
    PubKeyHash,
    /// p2wsh
    WitnessV0PubKeyHash,
    /// p2sh
    ScriptHash,
    /// p2wsh
    WitnessV0ScriptHash,
}

#[derive(PartialEq, Eq, Clone, Debug, Default, Serialize, Deserialize)]
pub struct BatchProof {
    /// All targets that'll be deleted
    pub targets: Vec<VarInt>,
    /// The inner hashes of a proof
    pub hashes: Vec<BlockHash>,
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct CompactLeafData {
    /// Header code tells the height of creating for this UTXO and whether it's a coinbase
    pub header_code: u32,
    /// The amount locked in this UTXO
    pub amount: u64,
    /// The type of the locking script for this UTXO
    pub spk_ty: ScriptPubkeyType,
}

/// Leaf data is the data that is hashed when adding to utreexo state. It contains validation
/// data and some commitments to make it harder to attack an utreexo-only node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeafData {
    /// A commitment to the block creating this utxo
    pub block_hash: BlockHash,
    /// The utxo's outpoint
    pub prevout: OutPoint,
    /// Header code is a compact commitment to the block height and whether or not this
    /// transaction is coinbase. It's defined as
    ///
    /// ```
    /// header_code: u32 = if transaction.is_coinbase() {
    ///     (block_height << 1 ) | 1
    /// } else {
    ///     block_height << 1
    /// };
    /// ```
    pub header_code: u32,
    /// The actual utxo
    pub utxo: TxOut,
}
/// The version tag to be prepended to the leafhash. It's just the sha512 hash of the string
/// `UtreexoV1` represented as a vector of [u8] ([85 116 114 101 101 120 111 86 49]).
/// The same tag is "5574726565786f5631" as a hex string.
pub const UTREEXO_TAG_V1: [u8; 64] = [
    0x5b, 0x83, 0x2d, 0xb8, 0xca, 0x26, 0xc2, 0x5b, 0xe1, 0xc5, 0x42, 0xd6, 0xcc, 0xed, 0xdd, 0xa8,
    0xc1, 0x45, 0x61, 0x5c, 0xff, 0x5c, 0x35, 0x72, 0x7f, 0xb3, 0x46, 0x26, 0x10, 0x80, 0x7e, 0x20,
    0xae, 0x53, 0x4d, 0xc3, 0xf6, 0x42, 0x99, 0x19, 0x99, 0x31, 0x77, 0x2e, 0x03, 0x78, 0x7d, 0x18,
    0x15, 0x6e, 0xb3, 0x15, 0x1e, 0x0e, 0xd1, 0xb3, 0x09, 0x8b, 0xdc, 0x84, 0x45, 0x86, 0x18, 0x85,
];

impl LeafData {
    pub fn get_leaf_hashes(&self) -> BitcoinNodeHash {
        let mut ser_utxo = vec![];
        let _ = self.utxo.consensus_encode(&mut ser_utxo);
        let leaf_hash = Sha512_256::new()
            .chain_update(UTREEXO_TAG_V1)
            .chain_update(UTREEXO_TAG_V1)
            .chain_update(self.block_hash)
            .chain_update(self.prevout.txid)
            .chain_update(self.prevout.vout.to_le_bytes())
            .chain_update(self.header_code.to_le_bytes())
            .chain_update(ser_utxo)
            .finalize();
        BitcoinNodeHash::from(leaf_hash.as_slice())
    }
}

pub trait LeafCache: Sync + Send + Sized + 'static {
    fn remove(&mut self, outpoint: &OutPoint) -> Option<LeafData>;
    fn insert(&mut self, outpoint: OutPoint, leaf_data: LeafData) -> bool;
    fn flush(&mut self) {}
    fn cache_size(&self) -> usize {
        0
    }
}

impl LeafCache for HashMap<OutPoint, LeafData> {
    fn remove(&mut self, outpoint: &OutPoint) -> Option<LeafData> {
        self.remove(outpoint)
    }
    fn insert(&mut self, outpoint: OutPoint, leaf_data: LeafData) -> bool {
        self.insert(outpoint, leaf_data);
        false
    }
}

fn get_input_leaf_hashes_new(file_path: &str) -> HashMap<TxIn, BitcoinNodeHash> {
    let file = File::open(file_path).unwrap();
    let reader = BufReader::new(file);

    let deserialized_struct: Vec<(TxIn, BitcoinNodeHash)> =
        serde_json::from_reader(reader).unwrap();
    let mut res: HashMap<TxIn, BitcoinNodeHash> = Default::default();
    for (txin, hash) in deserialized_struct {
        res.insert(txin, hash);
    }
    res
}

fn get_block_heights(data_path: &str) -> Result<Vec<u64>, Box<dyn Error>> {
    let mut heights = Vec::new();
    let re = Regex::new(r"^block-(\d+)txs$").unwrap();

    for entry in fs::read_dir(data_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(folder_name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(caps) = re.captures(folder_name) {
                    if let Some(num_match) = caps.get(1) {
                        if let Ok(height) = num_match.as_str().parse::<u64>() {
                            heights.push(height);
                        } else {
                            eprintln!("Warning: Couldn't parse height from '{}'", folder_name);
                        }
                    }
                }
            }
        }
    }
    Ok(heights)
}

fn process_block(
    pollard: &mut Pollard,
    block: &Block,
    height: u32,
    input_leaf_hashes: &mut HashMap<TxIn, BitcoinNodeHash>,
    tx_count: u64,
) {
    let mut inputs = Vec::new();
    let mut utxos = Vec::new();
    for tx in block.txdata.iter() {
        let txid = tx.compute_txid();
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
        for (idx, output) in tx.output.iter().enumerate() {
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
    println!(
        "Pollard: leaves: {}, roots: {}",
        pollard.leaves,
        pollard.get_roots().len()
    );
    let flagged_pollard = pollard.fake_modify(&utxos, &inputs);
    for root in flagged_pollard.get_roots() {
        println!("Root: {}", root.used.get());
    }
    println!(
        "Flagged pollard: leaves: {}, roots: {}",
        flagged_pollard.leaves,
        flagged_pollard.get_roots().len()
    );
    let stripped_pollard = flagged_pollard.get_stripped_pollard();
    println!(
        "Stripped pollard: leaves: {}, roots: {}",
        stripped_pollard.leaves,
        stripped_pollard.get_roots().len()
    );
    let file = File::create(format!(
        "processed-acc-data/block-{tx_count}txs/acc-before.txt"
    ))
    .unwrap();
    stripped_pollard.serialize(file).unwrap();
}

fn write_input_leaf_hashes(input_leaf_hashes: &HashMap<TxIn, BitcoinNodeHash>, tx_count: u64) {
    let leafs_pairs = input_leaf_hashes
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect::<Vec<_>>();
    let file = File::create(format!(
        "processed-acc-data/block-{tx_count}txs/input_leaf_hashes.txt"
    ))
    .unwrap();
    let writer = BufWriter::new(file);
    to_writer_pretty(writer, &leafs_pairs).unwrap();
}

fn read_height_from_file(file_path: &str) -> u32 {
    // let file = File::open(file_path).unwrap();
    std::fs::read_to_string(file_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Program description
#[derive(Parser)]
struct Cli {
    /// Optional exact value
    #[arg(long)]
    exact: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    let mut available_tx_counts = get_block_heights("acc-data/").unwrap();
    available_tx_counts.sort();
    available_tx_counts = vec![2, 3, 4, 5, 6, 13];
    if args.exact.is_some() {
        available_tx_counts = vec![args.exact.unwrap()];
    }
    for tx_count in available_tx_counts {
        let _ = std::fs::create_dir(format!("processed-acc-data/block-{tx_count}txs"));
        let block_path: String = format!("acc-data/block-{tx_count}txs/block.txt");
        println!("Processing block: {block_path}");
        println!("Current directory: {:#?}", std::env::current_dir().unwrap());

        // debug
        for entry in fs::read_dir(".")? {
            let entry = entry?;
            println!("{}", entry.path().display());
        }
        for entry in fs::read_dir("./acc-data")? {
            let entry = entry?;
            println!("{}", entry.path().display());
        }
        for entry in fs::read_dir("./acc-data/block-1txs")? {
            let entry = entry?;
            println!("{}", entry.path().display());
        }
        // end of debug

        let block: Block = bitcoin::consensus::deserialize(
            &fs::read(&block_path).expect("Failed to read block path"),
        )
        .unwrap();
        let height_path = format!("acc-data/block-{tx_count}txs/block-height.txt");
        let height: u32 = read_height_from_file(&height_path);
        println!("Calculated height: {height}");
        let acc_before_path: String = format!("acc-data/block-{tx_count}txs/acc-beffore.txt");

        let input_leaf_hashes_path: String =
            format!("acc-data/block-{tx_count}txs/input-leaf-hashes.txt");
        let serialized_acc_before = fs::read(&acc_before_path).unwrap();
        let mut acc_before = Pollard::deserialize(Cursor::new(&serialized_acc_before)).unwrap();

        let mut input_leaf_hashes: HashMap<TxIn, BitcoinNodeHash> =
            get_input_leaf_hashes_new(&input_leaf_hashes_path);
        write_input_leaf_hashes(&input_leaf_hashes.clone(), tx_count);
        process_block(
            &mut acc_before,
            &block,
            height,
            &mut input_leaf_hashes,
            tx_count,
        );
        std::fs::copy(
            format!("acc-data/block-{tx_count}txs/block.txt"),
            format!("processed-acc-data/block-{tx_count}txs/block.txt"),
        )
        .unwrap();
        std::fs::copy(
            format!("acc-data/block-{tx_count}txs/acc-after.txt"),
            format!("processed-acc-data/block-{tx_count}txs/acc-after.txt"),
        )
        .unwrap();
        std::fs::copy(
            format!("acc-data/block-{tx_count}txs/block-height.txt"),
            format!("processed-acc-data/block-{tx_count}txs/block-height.txt"),
        )
        .unwrap();
    }
    Ok(())
}

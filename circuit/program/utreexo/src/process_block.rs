use bitcoin::{Block, OutPoint, TxIn, Txid};
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;
use bitcoin_io::{Write, ErrorKind, Result};
use bitcoin::consensus::Encodable;
use bitcoin_hashes::Hash;

use sha2::{digest, Digest, Sha256};

use crate::btc_structs::{BatchProof, LeafData};

struct VecWriter<'a>(&'a mut Vec<u8>);

impl Write for VecWriter<'_> {
    /// Writes `buf` into this writer, returning how many bytes were written.
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Flushes this output stream, ensuring that all intermediately buffered contents
    /// reach their destination.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    /// Attempts to write an entire buffer into this writer.
    #[inline]
    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(ErrorKind::UnexpectedEof.into()),
                Ok(len) => buf = &buf[len..],
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl<'a> VecWriter<'a> {
    fn new(v: &'a mut Vec<u8>) -> Self {
        VecWriter(v)
    }
}
    

fn compute_txid(tx: &bitcoin::Transaction) -> bitcoin::Txid {
    let mut tx_bytes = Vec::new();
    let mut writer = VecWriter::new(&mut tx_bytes);
    
    tx.version.consensus_encode(&mut tx_bytes).expect("engines don't error");
    tx.input.consensus_encode(&mut tx_bytes).expect("engines don't error");
    tx.output.consensus_encode(&mut tx_bytes).expect("engines don't error");
    tx.lock_time.consensus_encode(&mut tx_bytes).expect("engines don't error");

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

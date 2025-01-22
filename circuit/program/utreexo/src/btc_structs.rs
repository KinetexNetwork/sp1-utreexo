// This whole file aims to make some structs from bitcoin and rustreexo crates friendly to
// sp1-zkvm. I will be happy to change them to some less hacky approach in the future.

use bitcoin::consensus::Encodable;
use bitcoin::{BlockHash, OutPoint, TxOut, VarInt};
use bitcoin_hashes::serde::{Deserialize, Serialize};
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use sha2::{Digest, Sha512_256};

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

impl LeafData {
    pub fn get_leaf_hashes(&self) -> BitcoinNodeHash {
        let mut ser_utxo = vec![];
        let _ = self.utxo.consensus_encode(&mut ser_utxo);
        let leaf_hash = Sha512_256::new()
            .chain_update(self.block_hash)
            .chain_update(self.prevout.txid)
            .chain_update(self.prevout.vout.to_le_bytes())
            .chain_update(self.header_code.to_le_bytes())
            .chain_update(ser_utxo)
            .finalize();
        BitcoinNodeHash::from(leaf_hash.as_slice())
    }
}

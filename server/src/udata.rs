// SPDX-License-Identifier: MIT

use bitcoin::consensus;
use bitcoin::consensus::encode::Error;
use bitcoin::consensus::Decodable;
use bitcoin::consensus::Encodable;
use bitcoin::Block;
use bitcoin::BlockHash;
use bitcoin::ScriptBuf;
use bitcoin::Txid;
use bitcoin::VarInt;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeafContext {
    #[allow(dead_code)]
    pub block_hash: BlockHash,
    pub txid: Txid,
    pub vout: u32,
    pub value: u64,
    pub pk_script: ScriptBuf,
    pub block_height: u32,
    pub median_time_past: u32,
    pub is_coinbase: bool,
}

/// Commitment of the leaf data, but in a compact way
///
/// The serialized format is:
/// [<header_code><amount><spk_type>]
///
/// The serialized header code format is:
///   bit 0 - containing transaction is a coinbase
///   bits 1-x - height of the block that contains the spent txout
///
/// It's calculated with:
///   header_code = <<= 1
///   if IsCoinBase {
///       header_code |= 1 // only set the bit 0 if it's a coinbase.
///   }
/// ScriptPubkeyType is the output's scriptPubkey, but serialized in a more efficient way
/// to save bandwidth. If the type is recoverable from the scriptSig, don't download the
/// scriptPubkey.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct CompactLeafData {
    /// Header code tells the height of creating for this UTXO and whether it's a coinbase
    pub header_code: u32,
    /// The amount locked in this UTXO
    pub amount: u64,
    /// The type of the locking script for this UTXO
    pub spk_ty: ScriptPubkeyType,
}

/// A recoverable scriptPubkey type, this avoids copying over data that are already
/// present or can be computed from the transaction itself.
/// An example is a p2pkh, the public key is serialized in the scriptSig, so we can just
/// grab it and hash to obtain the actual scriptPubkey. Since this data is committed in
/// the Utreexo leaf hash, it is still authenticated
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

impl Decodable for ScriptPubkeyType {
    fn consensus_decode<R: bitcoin::io::Read + ?Sized>(
        reader: &mut R,
    ) -> Result<ScriptPubkeyType, bitcoin::consensus::encode::Error> {
        let ty = u8::consensus_decode(reader)?;
        match ty {
            0x00 => Ok(ScriptPubkeyType::Other(Box::consensus_decode(reader)?)),
            0x01 => Ok(ScriptPubkeyType::PubKeyHash),
            0x02 => Ok(ScriptPubkeyType::WitnessV0PubKeyHash),
            0x03 => Ok(ScriptPubkeyType::ScriptHash),
            0x04 => Ok(ScriptPubkeyType::WitnessV0ScriptHash),
            _ => Err(bitcoin::consensus::encode::Error::ParseFailed(
                "Invalid script type",
            )),
        }
    }
}

impl Encodable for ScriptPubkeyType {
    fn consensus_encode<W: bitcoin::io::Write + ?Sized>(
        &self,
        writer: &mut W,
    ) -> Result<usize, bitcoin::io::Error> {
        let mut len = 1;

        match self {
            ScriptPubkeyType::Other(script) => {
                00_u8.consensus_encode(writer)?;
                len += script.consensus_encode(writer)?;
            }
            ScriptPubkeyType::PubKeyHash => {
                0x01_u8.consensus_encode(writer)?;
            }
            ScriptPubkeyType::WitnessV0PubKeyHash => {
                0x02_u8.consensus_encode(writer)?;
            }
            ScriptPubkeyType::ScriptHash => {
                0x03_u8.consensus_encode(writer)?;
            }
            ScriptPubkeyType::WitnessV0ScriptHash => {
                0x04_u8.consensus_encode(writer)?;
            }
        }
        Ok(len)
    }
}

/// BatchProof serialization defines how the utreexo accumulator proof will be
/// serialized both for i/o.
///
/// Note that this serialization format differs from the one from
/// github.com/mit-dci/utreexo/accumulator as this serialization method uses
/// varints and the one in that package does not.  They are not compatible and
/// should not be used together.  The serialization method here is more compact
/// and thus is better for wire and disk storage.
///
/// The serialized format is:
/// [<target count><targets><proof count><proofs>]
///
/// All together, the serialization looks like so:
/// Field          Type       Size
/// target count   varint     1-8 bytes
/// targets        []uint64   variable
/// hash count     varint     1-8 bytes
/// hashes         []32 byte  variable
#[derive(PartialEq, Eq, Clone, Debug, Default)]
pub struct BatchProof {
    /// All targets that'll be deleted
    pub targets: Vec<VarInt>,
    /// The inner hashes of a proof
    pub hashes: Vec<BlockHash>,
}

/// UData contains data needed to prove the existence and validity of all inputs
/// for a Bitcoin block.  With this data, a full node may only keep the utreexo
/// roots and still be able to fully validate a block.
#[derive(PartialEq, Eq, Clone, Debug, Default)]
pub struct UData {
    /// All the indexes of new utxos to remember.
    pub remember_idx: Vec<u64>,
    /// AccProof is the utreexo accumulator proof for all the inputs.
    pub proof: BatchProof,
    /// LeafData are the tx validation data for every input.
    pub leaves: Vec<CompactLeafData>,
}

/// A block plus some udata
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct UtreexoBlock {
    /// A actual block
    pub block: Block,
    /// The utreexo specific data
    pub udata: Option<UData>,
}

impl Decodable for UtreexoBlock {
    fn consensus_decode<R: bitcoin::io::Read + ?Sized>(
        reader: &mut R,
    ) -> Result<Self, consensus::encode::Error> {
        let block = Block::consensus_decode(reader)?;

        if let Err(Error::Io(_remember)) = VarInt::consensus_decode(reader) {
            return Ok(block.into());
        };

        let n_positions = VarInt::consensus_decode(reader)?;
        let mut targets = vec![];
        for _ in 0..n_positions.0 {
            let pos = VarInt::consensus_decode(reader)?;
            targets.push(pos);
        }

        let n_hashes = VarInt::consensus_decode(reader)?;
        let mut hashes = vec![];
        for _ in 0..n_hashes.0 {
            let hash = BlockHash::consensus_decode(reader)?;
            hashes.push(hash);
        }

        let n_leaves = VarInt::consensus_decode(reader)?;
        let mut leaves = vec![];
        for _ in 0..n_leaves.0 {
            let header_code = u32::consensus_decode(reader)?;
            let amount = u64::consensus_decode(reader)?;
            let spk_ty = ScriptPubkeyType::consensus_decode(reader)?;

            leaves.push(CompactLeafData {
                header_code,
                amount,
                spk_ty,
            });
        }

        Ok(Self {
            block,
            udata: Some(UData {
                remember_idx: vec![],
                proof: BatchProof { targets, hashes },
                leaves,
            }),
        })
    }
}

impl Encodable for UtreexoBlock {
    fn consensus_encode<W: bitcoin::io::Write + ?Sized>(
        &self,
        writer: &mut W,
    ) -> Result<usize, bitcoin::io::Error> {
        let mut len = self.block.consensus_encode(writer)?;

        if let Some(udata) = &self.udata {
            len += VarInt(udata.remember_idx.len() as u64).consensus_encode(writer)?;
            len += VarInt(udata.proof.targets.len() as u64).consensus_encode(writer)?;
            for target in &udata.proof.targets {
                len += target.consensus_encode(writer)?;
            }

            len += VarInt(udata.proof.hashes.len() as u64).consensus_encode(writer)?;
            for hash in &udata.proof.hashes {
                len += hash.consensus_encode(writer)?;
            }

            len += VarInt(udata.leaves.len() as u64).consensus_encode(writer)?;
            for leaf in &udata.leaves {
                len += leaf.header_code.consensus_encode(writer)?;
                len += leaf.amount.consensus_encode(writer)?;
                len += leaf.spk_ty.consensus_encode(writer)?;
            }
        }

        Ok(len)
    }
}

impl From<UtreexoBlock> for Block {
    fn from(block: UtreexoBlock) -> Self {
        block.block
    }
}

impl From<Block> for UtreexoBlock {
    fn from(block: Block) -> Self {
        UtreexoBlock { block, udata: None }
    }
}

pub mod bitcoin_leaf_data {
    use bitcoin::consensus::Decodable;
    use bitcoin::consensus::Encodable;
    use bitcoin::Amount;
    use bitcoin::TxOut;
    use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    use serde::Deserialize;
    use serde::Serialize;
    use sha2::Digest;
    use sha2::Sha512_256;

    use super::LeafContext;

    /// Leaf data is the data that is hashed when adding to utreexo state. It contains validation
    /// data and some commitments to make it harder to attack an utreexo-only node.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct BitcoinLeafData {
        /// The actual utxo
        pub utxo: TxOut,
    }

    impl BitcoinLeafData {
        pub fn get_leaf_hashes(leaf: &LeafContext) -> BitcoinNodeHash {
            let leaf_data = BitcoinLeafData::from(leaf.clone());
            leaf_data.compute_hash()
        }

        pub fn compute_hash(&self) -> BitcoinNodeHash {
            let mut ser_utxo = vec![];
            let _ = self.utxo.consensus_encode(&mut ser_utxo);
            let leaf_hash = Sha512_256::new().chain_update(ser_utxo).finalize();
            BitcoinNodeHash::from(leaf_hash.as_slice())
        }
    }

    impl Decodable for BitcoinLeafData {
        fn consensus_decode<R: bitcoin::io::Read + ?Sized>(
            reader: &mut R,
        ) -> Result<Self, bitcoin::consensus::encode::Error> {
            Self::consensus_decode_from_finite_reader(reader)
        }

        fn consensus_decode_from_finite_reader<R: bitcoin::io::Read + ?Sized>(
            reader: &mut R,
        ) -> Result<Self, bitcoin::consensus::encode::Error> {
            let utxo = TxOut::consensus_decode(reader)?;
            Ok(BitcoinLeafData { utxo })
        }
    }

    impl Encodable for BitcoinLeafData {
        fn consensus_encode<W: bitcoin::io::Write + ?Sized>(
            &self,
            writer: &mut W,
        ) -> Result<usize, bitcoin::io::Error> {
            let mut len = 0;
            len += self.utxo.consensus_encode(writer)?;
            Ok(len)
        }
    }

    impl From<LeafContext> for BitcoinLeafData {
        fn from(value: LeafContext) -> Self {
            BitcoinLeafData {
                utxo: TxOut {
                    value: Amount::from_sat(value.value),
                    script_pubkey: value.pk_script,
                },
            }
        }
    }
}

pub use bitcoin_leaf_data::BitcoinLeafData as LeafData;

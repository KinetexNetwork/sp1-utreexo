pub mod btc_structs;
pub mod process_block;

// re‐export the bits you’ll actually need in your script crate:
pub use btc_structs::BatchProof;
pub use btc_structs::LeafData;
pub use btc_structs::ScriptPubkeyType;
pub use btc_structs::UTREEXO_TAG_V1;
pub use process_block::process_block;

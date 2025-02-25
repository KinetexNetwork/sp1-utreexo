use std::collections::HashMap;

use bitcoin::Block;
use bitcoin::TxIn;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use serde::Deserialize;
use serde::Serialize;
use sp1_sdk::utils;
use sp1_sdk::EnvProver;
use sp1_sdk::ProverClient;
use sp1_sdk::SP1ProofWithPublicValues;
use sp1_sdk::SP1ProvingKey;
use sp1_sdk::SP1Stdin;

pub fn run_circuit(
    block: &Block,
    stripped_pollard: Pollard,
    input_leaf_hashes: &HashMap<TxIn, BitcoinNodeHash>,
    height: u32,
    prover_client: &EnvProver,
    proving_key: &SP1ProvingKey,
) -> SP1ProofWithPublicValues {
    let mut stdin = SP1Stdin::new();

    stdin.write::<Block>(&block);
    stdin.write::<u32>(&height);
    stdin.write::<Pollard>(&stripped_pollard);
    stdin.write::<HashMap<TxIn, BitcoinNodeHash>>(&input_leaf_hashes);

    let proof = prover_client
        .prove(&proving_key, &stdin)
        .run()
        .expect("failed to generate proof");
    proof
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ProofStorage {
    proofs_map: HashMap<u32, SP1ProofWithPublicValues>,
}

impl ProofStorage {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn keys(&self) -> Vec<u32> {
        self.proofs_map
            .keys()
            .map(|height| height.clone())
            .collect()
    }
    pub fn add_proof(&mut self, height: u32, proof: SP1ProofWithPublicValues) {
        self.proofs_map.insert(height, proof);
    }
    pub fn get_proof(&self, height: u32) -> Option<SP1ProofWithPublicValues> {
        self.proofs_map.get(&height).cloned()
    }
}

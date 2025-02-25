use std::collections::HashMap;

use bitcoin::Block;
use bitcoin::TxIn;
use log::info;
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
use std::fs;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofStorage {
    proofs_map: HashMap<u32, SP1ProofWithPublicValues>,
    storage_dir: String,
}

impl ProofStorage {
    pub fn new(storage_dir: String) -> Self {
        fs::create_dir_all(&storage_dir).expect("Failed to create storage directory");

        Self {
            proofs_map: Default::default(),
            storage_dir,
        }
    }

    pub fn keys(&self) -> Vec<u32> {
        self.proofs_map.keys().copied().collect()
    }

    pub fn add_proof(&mut self, height: u32, proof: SP1ProofWithPublicValues) {
        let proof_path = self.proof_path(height);
        self.proofs_map.insert(height, proof.clone());
        let _ = fs::File::create(&proof_path).expect("failed to create file");
        info!("Created file {proof_path}");
        let _ = proof.save(proof_path);
    }

    pub fn get_proof(&mut self, height: u32) -> Option<SP1ProofWithPublicValues> {
        if let Some(proof) = self.proofs_map.get(&height) {
            return Some(proof.clone());
        }
        self.get_proof_from_disk(height)
    }

    fn get_proof_from_disk(&self, height: u32) -> Option<SP1ProofWithPublicValues> {
        std::panic::catch_unwind(|| SP1ProofWithPublicValues::load(self.proof_path(height)).ok())
            .ok()?
    }

    fn proof_path(&self, height: u32) -> String {
        format!("{}/{}.proof", self.storage_dir, height)
    }
}

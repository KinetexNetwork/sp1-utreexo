use log::info;
use sp1_sdk::{SP1Proof, SP1VerificationKey};

struct Config {
    pub bridge_url: String,
    pub bridge_port: u16,
}

impl Config {
    pub fn new() -> Self {
        Self {
            bridge_url: "localhost".to_string(),
            bridge_port: 3000,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}:{}/", self.bridge_url, self.bridge_port)
    }
}

struct Verifier {
    config: Config,
}

impl Verifier {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(&self) {
        info!("Running verifier");
        let sp1_verification_key = self.get_sp1_verification_key().await;

        loop {
            let sp1_proof = self.get_sp1_proof().await;
        }
    }

    async fn get_sp1_verification_key(&self) -> SP1VerificationKey {
        let request_path = format!("{}{}", self.config.base_url(), "/sp1/verification-key");
        let response = reqwest::get(&request_path).await.unwrap();
        response.json().await.unwrap()
    }

    async fn get_sp1_proof(&self) -> SP1Proof {
        let request_path = format!("{}{}", self.config.base_url(), "/sp1/proof");
        let response = reqwest::get(&request_path).await.unwrap();
        response.json().await.unwrap()
    }
}



fn main() {
    let config = Config::new();
    println!("Hello, world!");
}

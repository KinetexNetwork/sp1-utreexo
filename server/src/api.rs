//SPDX-License-Identifier: MIT

//! This is a simple REST API that can be used to query Utreexo data. You can get the roots
//! of the accumulator, get a proof for a leaf, and get a block and the associated UData.
use std::str::FromStr;
use std::sync::Arc;

use actix_cors::Cors;
use actix_web::web;
use actix_web::App;
use actix_web::HttpResponse;
use actix_web::HttpServer;
use actix_web::Responder;
use bitcoin::consensus::deserialize;
use bitcoin::hashes::Hash;
use bitcoin::Block;
use bitcoin::BlockHash;
use bitcoin::Txid;
use bitcoincore_rpc::jsonrpc::serde_json::json;
use futures::channel::mpsc::Sender;
use futures::lock::Mutex;
use futures::SinkExt;
use log::info;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::proof::Proof;
use serde::Deserialize;
use serde::Serialize;

use crate::chainview::ChainView;
use crate::db::Requests;
use crate::db::Responses;
use crate::udata::CompactLeafData;
use crate::udata::UtreexoBlock;

type SenderCh = Mutex<
    Sender<(
        Requests,
        futures::channel::oneshot::Sender<Result<Responses, String>>,
    )>,
>;

/// This is the state of the actix-web server that will be passed as reference by each
/// callback function. It contains a sender that can be used to send requests to the prover.
struct AppState {
    /// Sender to send requests to the prover.
    sender: SenderCh,
    view: Arc<ChainView>,
}

/// This function is used to send a request to the prover and wait for the response, and
/// return the response or an error.
async fn perform_request(
    data: &web::Data<AppState>,
    request: Requests,
) -> Result<Responses, String> {
    let (sender, receiver) = futures::channel::oneshot::channel();
    data.sender
        .lock()
        .await
        .send((request, sender))
        .await
        .unwrap();
    receiver.await.unwrap()
}

async fn get_sp1_verification_key(data: web::Data<AppState>) -> impl Responder {
    info!("Getting SP1 verification key request");
    let res = perform_request(&data, Requests::GetSP1VerificationKey).await;
    info!("Got response from to SP1 verification key request from db");
    match res {
        Ok(Responses::SP1VerificationKey(key)) => HttpResponse::Ok().json(json!({
            "error": null,
            "data": key,
        })),
        Ok(_) => HttpResponse::InternalServerError().json(json!({
            "error": "Invalid response",
            "data": null
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({
            "error": e,
            "data": null
        })),
    }
}

async fn get_sp1_proof(hash: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    info!("Getting SP1 proof request");
    let hash = hash.into_inner();
    let hash = BlockHash::from_str(&hash);

    let res = perform_request(&data, Requests::GetSP1Proof(hash.unwrap())).await;
    info!("Got response to SP1 proof request from db");
    match res {
        Ok(Responses::SP1Proof(proof)) => HttpResponse::Ok().json(json!({
            "error": null,
            "data": proof,
        })),
        Ok(_) => HttpResponse::InternalServerError().json(json!({
            "error": "Invalid response",
            "data": null
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({
            "error": e,
            "data": null
        })),
    }
}

/// This function creates the actix-web server and returns a future that can be awaited.
pub async fn create_api(
    request: Sender<(
        Requests,
        futures::channel::oneshot::Sender<Result<Responses, String>>,
    )>,
    view: Arc<ChainView>,
    host: &str,
) -> std::io::Result<()> {
    let app_state = web::Data::new(AppState {
        sender: Mutex::new(request),
        view,
    });
    HttpServer::new(move || {
        let cors = Cors::permissive();
        App::new()
            .wrap(cors)
            .app_data(app_state.clone())
            .route("/sp1/{hash}", web::get().to(get_sp1_proof))
            .route("/sp1/verification-key", web::get().to(get_sp1_verification_key))
    })
    .bind(host)?
    .run()
    .await
}
/// The proof serialization by serde-json is not very nice, because it serializes byte-arrays
/// as a array of integers. This struct is used to serialize the proof in a nicer way.
#[derive(Clone, Serialize, Deserialize)]
struct JsonProof {
    targets: Vec<u64>,
    hashes: Vec<String>,
}

impl From<Proof> for JsonProof {
    fn from(proof: Proof) -> Self {
        let targets = proof.targets;
        let mut hashes = Vec::new();
        for hash in proof.hashes {
            hashes.push(hash.to_string());
        }
        JsonProof { targets, hashes }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct UBlock {
    block: Block,
    proof: JsonProof,
    leaf_data: Vec<CompactLeafData>,
}

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
impl From<UtreexoBlock> for UBlock {
    fn from(block: UtreexoBlock) -> Self {
        let proof = block.udata.as_ref().unwrap().proof.clone();
        let proof = Proof {
            hashes: proof
                .hashes
                .iter()
                .map(|x| BitcoinNodeHash::from(x.to_raw_hash().to_byte_array()))
                .collect(),
            targets: proof.targets.iter().map(|x| x.0).collect(),
        }
        .into();

        let leaves = block.udata.clone().unwrap().leaves.clone();
        let block = block.block;

        Self {
            block,
            proof,
            leaf_data: leaves,
        }
    }
}

use std::env;
use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

use actix_rt::signal::ctrl_c;
use clap::Parser;
use futures::channel::mpsc::channel;
use log::info;
use log::warn;

use crate::api;
use crate::block_index::BlocksIndex;
use crate::blockfile::BlockFile;
use crate::chainview;
use crate::cli::CliArgs;
use crate::get_chain_provider;
use crate::init_logger;
use crate::leaf_cache::DiskLeafStorage;
use crate::node;
use crate::node::Node;
use crate::prover;
use crate::subdir;

pub fn run_bridge() -> anyhow::Result<()> {
    let cli_options = CliArgs::parse();
    fs::DirBuilder::new()
        .recursive(true)
        .create(subdir(""))
        .unwrap();

    // Initialize the logger
    init_logger(
        Some(&subdir("debug.log")),
        simplelog::LevelFilter::Info,
        true,
    );

    // to keep track of the current chain state and speed up replying to headers requests
    // from peers.
    let store = kv::Store::new(kv::Config {
        path: subdir("chain_view").into(),
        temporary: false,
        use_compression: false,
        flush_every_ms: None,
        cache_capacity: None,
        segment_size: None,
    })
    .expect("Failed to open chainview database");

    // Chainview is a collection of metadata about the chain, like tip and block
    // indexes. It's stored in a key-value database.
    let view = chainview::ChainView::new(store);
    let view = Arc::new(view);

    // This database stores some useful information about the blocks, but not
    // the blocks themselves
    let index_store = BlocksIndex {
        database: kv::Store::new(kv::Config {
            path: subdir("index/").into(),
            temporary: false,
            use_compression: false,
            flush_every_ms: Some(1000),
            cache_capacity: Some(1_000_000),
            segment_size: None,
        })
        .unwrap(),
    };

    // Put it into an Arc so we can share it between threads
    let index_store = Arc::new(index_store);

    // The prover needs some way to pull blocks from a trusted source, we can use anything
    // implementing the [Blockchain] trait, for example a bitcoin core node or an esplora
    // instance.
    let client = get_chain_provider()?;

    // Create a prover, this module will download blocks from the bitcoin core
    // node and save them to disk. It will also create proofs for the blocks
    // and save them to disk.
    let leaf_data = DiskLeafStorage::new(&subdir("leaf_data"));

    // a signal used to stop the prover
    let kill_signal = Arc::new(Mutex::new(false));

    //let leaf_data = HashMap::new(); // In-memory leaf storage,
    // faster than leaf_data but uses more memory

    let (block_notifier_tx, block_notifier_rx) = std::sync::mpsc::channel();
    let snapshot_rate = cli_options.save_proofs_after.unwrap_or(50000);
    info!("snapshot rate = {}", snapshot_rate);
    let mut prover = prover::Prover::new(
        client,
        index_store.clone(),
        view.clone(),
        leaf_data,
        cli_options.initial_state_path.map(Into::into),
        cli_options.start_height,
        cli_options.acc_snapshot_every_n_blocks,
        kill_signal.clone(),
        snapshot_rate,
        block_notifier_tx,
    );

    // Keep the prover running in the background, it will download blocks and
    // create proofs for them as they are mined.
    info!("Running prover");
    std::thread::spawn(move || {
        actix_rt::System::new().block_on(async {
            let _ = ctrl_c().await;
            warn!("Received a stop signal");
            *kill_signal.lock().unwrap() = true;
        })
    });

    prover.keep_up()
}

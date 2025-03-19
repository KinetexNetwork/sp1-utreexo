use bitcoin::Block;
use bitcoin::TxIn;
use clap::Parser;
use input_generator_lib::*;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self};
use std::io::Cursor;
use std::path::Path;

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
    let data_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("acc-data")
        .to_str()
        .unwrap()
        .to_string();
    let output_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("processed-acc-data")
        .to_str()
        .unwrap()
        .to_string();
    let mut available_tx_counts = get_block_heights(&data_path).unwrap();
    available_tx_counts.sort();
    available_tx_counts = vec![5];
    if args.exact.is_some() {
        available_tx_counts = vec![args.exact.unwrap()];
    }
    for tx_count in available_tx_counts {
        let path_data = data_path.clone();
        let _ = std::fs::create_dir(format!("processed-acc-data/block-{tx_count}txs"));
        let block_path: String = format!("{path_data}/block-{tx_count}txs/block.txt");
        println!("Processing block: {block_path}");
        println!("Current directory: {:#?}", std::env::current_dir().unwrap());

        // debug
        println!("Current directory contents:");
        for entry in fs::read_dir(data_path.clone())? {
            let entry = entry?;
            println!("{}", entry.path().display());
        }
        println!("acc-data:");
        for entry in fs::read_dir(data_path.clone())? {
            let entry = entry?;
            println!("{}", entry.path().display());
        }
        let block_directory = format!("{}/block-5txs", data_path.clone());
        println!("{}", block_directory);

        for entry in fs::read_dir(&block_directory)? {
            let entry = entry?;
            println!("{}", entry.path().display());
        }
        // end of debug

        let block: Block = bitcoin::consensus::deserialize(
            &fs::read(&block_path).expect("Failed to read block path"),
        )
        .unwrap();
        let height_path = format!(
            "{}/block-{}txs/block-height.txt",
            data_path.clone(),
            tx_count
        );
        let height: u32 = read_height_from_file(&height_path);
        println!("Calculated height: {height}");
        let acc_before_path: String =
            format!("{}/block-{}txs/acc-before.txt", data_path.clone(), tx_count);

        let input_leaf_hashes_path: String =
            format!("{}/block-{}txs/input_leaf_hashes.txt", data_path, tx_count);
        println!("input_leaf_hashes_path: {input_leaf_hashes_path}");
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
            format!("{}/block-{}txs/block.txt", data_path, tx_count),
            format!("{}/block-{}txs/block.txt", output_path, tx_count),
        )
        .unwrap();
        std::fs::copy(
            format!("{}/block-{}txs/acc-after.txt", data_path.clone(), tx_count),
            format!(
                "{}/block-{}txs/acc-after.txt",
                output_path.clone(),
                tx_count
            ),
        )
        .unwrap();
        std::fs::copy(
            format!("{}/block-{}txs/block-height.txt", data_path, tx_count),
            format!("{}/block-{}txs/block-height.txt", output_path, tx_count),
        )
        .unwrap();
    }
    Ok(())
}

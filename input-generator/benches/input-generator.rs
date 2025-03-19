use bitcoin::consensus;
use bitcoin::Block;
use bitcoin::TxIn;
use input_generator_lib::*;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn bench_main(exact: usize) -> Result<(), Box<dyn Error>> {
    // Set up paths
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

    // Only process the given exact block folder
    let tx_count = exact;

    let _ = fs::create_dir(format!("processed-acc-data/block-{tx_count}txs"));
    let block_path = format!("{data_path}/block-{tx_count}txs/block.txt");
    println!("Processing block: {}", block_path);
    println!("Current directory: {:?}", std::env::current_dir()?);

    // Debug: list acc-data folder contents
    for entry in fs::read_dir(&data_path)? {
        let entry = entry?;
        println!("{}", entry.path().display());
    }

    let block: Block =
        consensus::deserialize(&fs::read(&block_path).expect("Failed to read block from disk"))
            .unwrap();

    let height_path = format!("{data_path}/block-{tx_count}txs/block-height.txt");
    let height: u32 = read_height_from_file(&height_path);
    println!("Calculated height: {}", height);

    let acc_before_path = format!("{data_path}/block-{tx_count}txs/acc-before.txt");
    let input_leaf_hashes_path = format!("{data_path}/block-{tx_count}txs/input_leaf_hashes.txt");
    println!("input_leaf_hashes_path: {}", input_leaf_hashes_path);

    let serialized_acc_before = fs::read(&acc_before_path).unwrap();
    let mut acc_before = Pollard::deserialize(Cursor::new(&serialized_acc_before)).unwrap();

    let mut input_leaf_hashes: HashMap<TxIn, BitcoinNodeHash> =
        get_input_leaf_hashes_new(&input_leaf_hashes_path);
    write_input_leaf_hashes(&input_leaf_hashes, tx_count as u64);

    process_block(
        &mut acc_before,
        &block,
        height,
        &mut input_leaf_hashes,
        tx_count as u64,
    );

    // Copy output files to the processed-acc-data folder
    fs::copy(
        format!("{data_path}/block-{tx_count}txs/block.txt"),
        format!("{output_path}/block-{tx_count}txs/block.txt"),
    )
    .unwrap();
    fs::copy(
        format!("{data_path}/block-{tx_count}txs/acc-after.txt"),
        format!("{output_path}/block-{tx_count}txs/acc-after.txt"),
    )
    .unwrap();
    fs::copy(
        format!("{data_path}/block-{tx_count}txs/block-height.txt"),
        format!("{output_path}/block-{tx_count}txs/block-height.txt"),
    )
    .unwrap();

    Ok(())
}

fn bench_main_wrapper(c: &mut criterion::Criterion) {
    let exact_values = [5, 100, 2400];
    for &val in &exact_values {
        c.bench_function(&format!("input-generator (exact = {})", val), |b| {
            b.iter(|| {
                bench_main(criterion::black_box(val)).unwrap();
            })
        });
    }
}

criterion::criterion_group!(benches, bench_main_wrapper);
criterion::criterion_main!(benches);

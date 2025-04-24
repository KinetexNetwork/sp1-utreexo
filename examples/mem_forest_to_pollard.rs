// An example that reads a serialized MemForest from disk and converts it
// into a "pruned" Pollard that only keeps the branches needed to update /
// prove a given set of leaves (typically the inputs of a block).
//
// Usage:
//   cargo run --example mem_forest_to_pollard -- <mem_forest_file> <inputs_file>
//
// * `mem_forest_file` – binary file produced by MemForest::serialize
// * `inputs_file`     – text file, one 64-char hex hash per line – the
//                       leaves you expect to delete.
//
// The example performs the following steps:
//   1. deserialises the MemForest
//   2. parses the list of leaf hashes to be deleted
//   3. asks the forest for a proof for those hashes
//   4. creates a Pollard skeleton from the forest roots
//   5. ingests the proof, keeping only the branches that end in the
//      target leaves
//   6. prints a few basic statistics

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::str::FromStr;

use rustreexo::accumulator::mem_forest::MemForest;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::pollard::Pollard;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ---------------------------------------------------------------------
    // Parse command-line arguments
    // ---------------------------------------------------------------------
    let mut args = std::env::args().skip(1);
    let forest_path = args
        .next()
        .expect("Usage: mem_forest_to_pollard <mem_forest_file> <inputs_file>");
    let inputs_path = args
        .next()
        .expect("Usage: mem_forest_to_pollard <mem_forest_file> <inputs_file>");

    // ---------------------------------------------------------------------
    // 1. Read & deserialize the MemForest
    // ---------------------------------------------------------------------
    let mut forest_bytes = Vec::new();
    File::open(&forest_path)?.read_to_end(&mut forest_bytes)?;
    let mut cursor = std::io::Cursor::new(forest_bytes);
    let mem_forest = MemForest::<BitcoinNodeHash>::deserialize(&mut cursor)?;

    // ---------------------------------------------------------------------
    // 2. Read the list of leaf hashes we want to delete / track
    // ---------------------------------------------------------------------
    let reader = BufReader::new(File::open(&inputs_path)?);
    let mut del_hashes = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let hash = BitcoinNodeHash::from_str(trimmed)?;
        del_hashes.push(hash);
    }

    if del_hashes.is_empty() {
        eprintln!("Warning: no hashes found in inputs file");
    }

    // ---------------------------------------------------------------------
    // 3. Obtain the proof for those leaves from the full forest
    // ---------------------------------------------------------------------
    let proof = mem_forest
        .prove(&del_hashes)
        .map_err(|e| format!("Failed to build proof: {e}"))?;
    let remember_positions = proof.targets.clone();

    // ---------------------------------------------------------------------
    // 4. Build a minimal Pollard (only roots)
    // ---------------------------------------------------------------------
    let roots: Vec<_> = mem_forest
        .get_roots()
        .iter()
        .map(|r| r.get_data())
        .collect();
    let mut pollard = Pollard::from_roots(roots.clone(), mem_forest.leaves);

    // ---------------------------------------------------------------------
    // 5. Ingest the proof, keeping the required branches
    // ---------------------------------------------------------------------
    pollard
        .ingest_proof(proof, &del_hashes, &remember_positions)
        .map_err(|e| format!("Failed to ingest proof: {e}"))?;

    // ---------------------------------------------------------------------
    // 6. Output a few stats to show that it worked
    // ---------------------------------------------------------------------
    println!("Loaded MemForest with {} leaves", mem_forest.leaves);
    println!("Tracking {} leaves (will be deleted)", del_hashes.len());
    println!("Pollard ready. Roots kept: {}", roots.len());

    Ok(())
}

// (The Pollard structure purposefully keeps its internal state private.
//  For the sake of this example we do **not** expose it.  The printed
//  statistics above are therefore limited to what is publicly
//  available.)

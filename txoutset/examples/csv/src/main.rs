use std::io::Write;

use clap::Parser;
use txoutset::{ComputeAddresses, Dump};

use simplelog::{Config, LevelFilter, SimpleLogger};

fn main()  {
    let logger = SimpleLogger::init(LevelFilter::Info, Config::default());
    
    let dump = Dump::new("../../../utxo_dump.dat", ComputeAddresses::No);
    let mut vec = Vec::new();
    for item in dump {
        vec.push(item);
    }

    println!("Dump collected");
    println!("Dump size: {}", vec.len());
}

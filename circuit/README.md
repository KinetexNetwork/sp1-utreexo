# UTREEXO SP1

## Introduction
This project aims to build a SP1 based circuit, which will prove mutations on [utreexo](https://bitcoinops.org/en/topics/utreexo/) 
data structure -- Merkle forest based accumulator of utxos -- bitcoin unspent transaction outputs.

## Prerequisites
- make sure you installed sp1:
```bash
curl -L https://sp1.succinct.xyz | bash
sp1up
```

Also, make sure that version of sp1 you installed don't conflict with sp1 crates we are using in this project. (It shouldn't, but in case of problems try to look at this versions)

## How to run

Run circuit and measure zk-cycles:
```bash
cd script
cargo run --release -- --execute
```

Measure time of generating prove on your machine:
```bash
cd script
cargo run --release -- --prove
```

Profiling:
```bash
cd script
TRACE_FILE=trace.log RUST_LOG=info cargo run --release -- --execute
cargo prove trace --elf ../program/utreexo/elf/riscv32im-succinct-zkvm-elf --trace trace.log
```


## How fast we are?

Our target-minimum is to make circuit consumining 4 billion zk-cycles on latest blocks.
Some target-good-enough can be 1 billion zk-cycles.
TODO: add results here
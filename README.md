# sp1-utreexo

This project aims to implement SP-1 powered zk-circuit for updating a utreexo accumulator. Currently, the project have the following components:

- `circuit` - zk-circuit for updating a utreexo accumulator.
- `server` - a bitcoin bridge node, which fetches block data from a bitcoin node for further passage it into the circuit.
- `rustreexo` - a rust implementation of utreexo accumulator with some zk-friendly modifications.
- `input-generator` - a tool to take fetched data from the `server` and prepare it for the `circuit`.

## Status

Project is WIP and here is the list of things we already have:

- [x] we already have a zk-circuit for updating a utreexo accumulator
- [x] we have a server which fetches block data from a bitcoin node and updates utreexo outside circuit
- [x] we have code to prepare data for the circuit
- [x] we implemented some robust performance optimizations for circuit

TODO:
- [ ] To make the program under `program/utreexo/src` runnable on native devices (for profiling purposes).
- [ ] To bring back the borrowing checker available within the program's crate.
- [ ] integrate circuit with the server
- [ ] add endpoints to server for getting utreexo proofs and utreexo roots for given height

## Prerequisites

1. Install sp1 riscv32 toolchain: `sp1up --c-toolchain` -- it installs toolchain without includes of riscv32 which is necessary for make the project compiles.
2. Install riscv toolchain on your system: `brew tap riscv-software-src/riscv && brew install riscv-gnu-toolchain`
3. Export headers into terminal session: `export SP1_CFLAGS="-I$(brew --prefix riscv-gnu-toolchain)/riscv64-unknown-elf/include"`
4. (Optional) Export riscv32 compiler into terminal session: `export CC_riscv32im_succinct_zkvm_elf=~/.sp1/bin/riscv32-unknown-elf-gcc`

## How to run

From very high level there are two steps:
- run `server` following server/README.md and wait it to start processing blocks (it should be around a minute)
- run `python3 run-end-to-end.py` from the root of the project

Run with profiling:
```bash
CFLAGS=$SP1_CFLAGS TRACE_FILE=output.json TRACE_SAMPLE_RATE=100 cargo run --release --bin utreexo -- --execute --exact 5
```

Building circuit:
```bash
CFLAGS=$SP1_CFLAGS cargo prove build
```

Run circuit without generating proof with checking that utreexo roots computed in circuit and out of circuit match:

```bash
CFLAGS=$SP1_CFLAGS cargo run --release --bin utreexo -- --execute --exact 5
```

Run with generating proof:

```bash
SP1_PROVER=network NETWORK_PRIVATE_KEY=$NETWORK_PRIVATE_KEY cargo r --release --bin utreexo -- --prove --exact 5
```

## Aknowledgements
`server` and `rustreexo` are based on the work of Davidson-Souza and mit-dci:
https://github.com/mit-dci/rustreexo
https://github.com/Davidson-Souza/rpc-utreexo-bridge

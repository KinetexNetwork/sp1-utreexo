
# sp1-utreexo

ZK-powered accumulator for Bitcoin UTXO sets using the Utreexo design and SP-1 zkVM circuits.
This project implements:
  - `utreexo/`: a native Rust runner and zk-circuit wiring (under development) to update an accumulator state.
  - `accumulator-service/`: an HTTP server to build, update and prune a Utreexo accumulator from Bitcoin data.

---

## Prerequisites

- Rust (1.60+) with Cargo
- Bitcoin Core (v0.21+) fully synced with `txindex=1` and `server=1` enabled:
  ```ini
  # ~/.bitcoin/bitcoin.conf
  server=1
  txindex=1
  rpcuser=<USER>
  rpcpassword=<PASS>
  ```
- `bitcoin-cli` on `$PATH` for RPC calls
- `jq` (for test-data scripts)
- [utxo-to-parquet](https://github.com/romanz/utxo-to-parquet) to convert Bitcoin Core dump to Parquet
- Environment variables:
  ```bash
  export BITCOIN_CORE_RPC_URL="http://127.0.0.1:8332"
  export BITCOIN_CORE_COOKIE_FILE="$HOME/.bitcoin/.cookie"
  ```

---

## Installation

```bash
# Clone repository
git clone https://github.com/your-org/sp1-utreexo.git
cd sp1-utreexo

# Build all components
cargo build --all --release

# (Optional) Run tests
cargo test --all
```

## Configuration

- Ensure `BITCOIN_CORE_RPC_URL` and `BITCOIN_CORE_COOKIE_FILE` are exported
- Place your Parquet UTXO dump at a known path (e.g. `$HOME/utxo.parquet`)

## Usage

### accumulator-service

An HTTP server to build and manage a Utreexo accumulator from a Parquet dump.

```bash
cd accumulator-service
cargo run --release
# Server listens at http://127.0.0.1:8080
```

Endpoints:
  - POST /build  `{ "parquet": "/path/to/utxo.parquet", "resume_from": null }`
    → initializes and builds accumulator state, producing `mem_forest.bin` and `block_hashes.bin` in the working directory
  - POST /pause  → pause ongoing build
  - POST /resume → resume paused build
  - POST /stop   → stop processing
  - GET  /status → get current build status
  - POST /update `{ "height": 680000 }` → apply a block update, updating `mem_forest.bin` and generating a fresh pruned `pollard.bin`
  - POST /dump   → write a pruned Pollard snapshot to `snapshot/`
  - POST /restore→ reload from last disk snapshot

### utreexo (native runner)

Process a block with a local accumulator via command line (native feature):

```bash
# Example: feed JSON input to the utreexo binary
cargo run --release -p utreexo --features native \
  < input.json > output.bin
```

Refer to `utreexo/src/main.rs` for expected JSON format and output encoding.

### Test Data Generation

Use the `extract_from_block.sh` script to create fixtures for UTXO outputs:

```bash
export BITCOIN_CORE_COOKIE_FILE="$HOME/.bitcoin/.cookie"
./extract_from_block.sh <TXID> <VOUT> <HEIGHT> > test-data/utxo_output.txt
```

## Testing

Run all Rust unit tests:

```bash
cargo test --all
```

## Project Structure

- `utreexo/` — Rust crate for accumulator logic and zk-circuit wiring
- `accumulator-service/` — HTTP API service for accumulator build & updates
- `extract_from_block.sh` — helper script for test fixtures
- `Roadmap.md` — project TODOs and future work

## Roadmap & Contributing

See [Roadmap.md](Roadmap.md) for known issues, performance goals, and planned features.
Contributions welcome via issues and pull requests.

---

_Circuit wiring and on‑chain verification live in `utreexo/` but aren’t hooked up yet. Stay tuned!_```
# sp1‑utreexo

_WIP: SP‑1 powered zk‑circuit for updating a Utreexo accumulator on Bitcoin._

This repo currently has two crates:

1. **utreexo/** — native Rust runner for accumulator updates (circuit wiring not hooked up yet)
2. **accumulator-service/** — HTTP service for building, updating, and pruning the Utreexo accumulator

---

## Prerequisites

0. **Bitcoin Core** (v0.21+), fully synced  
   ```bash
   brew install bitcoin
   ```  
   Edit `$HOME/.bitcoin/bitcoin.conf`:
   ```
   server=1
   txindex=1
   rpcuser=<USER>
   rpcpassword=<PASS>
   ```  
   Start and sync:
   ```bash
   bitcoind -daemon
   ```

1. **utxo‑to‑parquet** (dump → Parquet converter)  
   ```bash
   git clone https://github.com/romanz/utxo-to-parquet.git $HOME/utxo-to-parquet
   cd $HOME/utxo-to-parquet
   cargo build --release
   ```

2. **RPC credentials**  
   ```bash
   export BITCOIN_CORE_RPC_URL="http://127.0.0.1:8332"
   export BITCOIN_CORE_COOKIE_FILE="$HOME/.bitcoin/.cookie"
   ```

---

## Quick Start

Assuming you have a Parquet dump of your UTXO set at `$HOME/utxo.parquet` (e.g. via `bitcoin-cli dumptxoutset` + an external converter), here’s how to get started:

1) Build all Rust components:
   ```bash
   cargo build --all --release
   ```

2) Start the accumulator-service HTTP server:
   ```bash
   cd accumulator-service
   cargo run --release
   ```
   The server will listen on `http://127.0.0.1:8080`.

3) Trigger the initial build (Parquet → accumulator):
   ```bash
   curl -X POST http://127.0.0.1:8080/build \
     -H "Content-Type: application/json" \
     -d '{"parquet": "/home/user/utxo.parquet", "resume_from": null}'
   ```
   This will create a `snapshot/` directory containing:
   - `block_hashes.bin`  
   - `mem_forest.bin`

4) Control the build process via HTTP:
   ```bash
   # Pause processing
   curl -X POST http://127.0.0.1:8080/pause

   # Resume processing
   curl -X POST http://127.0.0.1:8080/resume

   # Stop processing
   curl -X POST http://127.0.0.1:8080/stop

   # Check status
   curl http://127.0.0.1:8080/status
   ```

5) Update or prune blocks:
   ```bash
   # Process a single block height
   curl -X POST http://127.0.0.1:8080/update \
     -H "Content-Type: application/json" \
     -d '{"height": 680000}'

   # Trigger a pollard prune snapshot (writes to snapshot/)
   curl -X POST http://127.0.0.1:8080/dump

   # Reload from disk snapshot
   curl -X POST http://127.0.0.1:8080/restore
   ```

---

_Circuit wiring and on‑chain verification live in `utreexo/` but aren’t hooked up yet. Stay tuned!_```
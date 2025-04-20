# sp1‑utreexo

_WIP: SP‑1 powered zk‑circuit for updating a Utreexo accumulator on Bitcoin._

This repo currently has two crates:

1. **script/** — takes a Parquet UTXO dump + block‑hashes and builds your initial accumulator state  
2. **utreexo/** — native Rust runner for accumulator updates (circuit wiring not hooked up yet)

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

Assume you clone the repo to `$HOME/Development/git/sp1-utreexo`:

```bash
export REPO_ROOT="$HOME/Development/git/sp1-utreexo"
cd "$REPO_ROOT"
```

1) **Build**  
   ```bash
   cargo build --all --release
   ```

2) **Dump & convert the UTXO set**  
   ```bash
   bitcoin-cli dumptxoutset $HOME/utxo.dump

   $HOME/utxo-to-parquet/target/release/utxo-to-parquet \
     -i $HOME/utxo.dump \
     -o $HOME/utxo.parquet
   ```

3) **Generate the initial accumulator state**  
   ```bash
   cd "$REPO_ROOT/script"
   cargo run --release -- $HOME/utxo.parquet $HOME/acc-out
   ```
   This will write:
   - `$HOME/acc-out/block_hashes.bin`  
   - `$HOME/acc-out/mem_forest.bin`  

4) **(Optional) Test a block update with the native runner**  
   Prepare a JSON file `input.json` matching the `AccumulatorInput` struct:
   ```json
   {
     "block":   <bitcoin::Block as JSON>,
     "height":  <u32>,
     "mem_forest": <Vec<u8> contents of mem_forest.bin>,
     "input_leaf_hashes": { "<TxIn JSON>": "<leaf‑hash hex>", … }
   }
   ```
   Then:
   ```bash
   cd "$REPO_ROOT/utreexo"
   cat input.json \
     | cargo run --release --features native \
     > roots.bin
   ```
   `roots.bin` will contain the updated accumulator roots as flat 32‑byte hashes.

---

_Circuit wiring and on‑chain verification live in `utreexo/` but aren’t hooked up yet. Stay tuned!_```
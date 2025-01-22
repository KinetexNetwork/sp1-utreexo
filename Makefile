CIRCUIT_DIR = circuit/program/utreexo/
BRIDGE_DIR = server/

build-esplora:
	cd $(CIRCUIT_DIR) && RUST_LOG=none,bridge=info cargo prove build && cd -
	cd $(BRIDGE_DIR) && RUST_LOG=none,bridge=info cargo build --release --features esplora --features api && cd -

run-esplora:
	RUST_LOG=none,bridge=info ESPLORA_URL=https://blockstream.info/api ./target/release/bridge

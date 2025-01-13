# All commands should work from the root of the repository
# Maybe we will need install rust and python before doing this:
# Rust installation: https://www.rust-lang.org/tools/install

# Install SP1. Maybe we ok with having different version
curl -L https://sp1.succinct.xyz | bash
sp1up -C 459fac7

# Verify SP1 installation
cargo-prove prove --version

# Build SP1 circuit. Must be built before building other parts
cd circuit/program/utreexo
cargo-prove prove build

# Build the rest of the app
cargo build --workspace --exclude btcx-program-utreexo --exclude utreexo-script

# Run unit tests
cargo test --workspace --exclude btcx-program-utreexo --exclude utreexo-script
    
# Install linter
rustup component add clippy

# Run linter
cargo fmt --all -- --check
cargo clippy -- -D warnings

# Run python tests
python3 run-end-to-end.py --test

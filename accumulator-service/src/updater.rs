//! Updater logic stub.
use anyhow::Result;

/// Update the accumulator with a new block at given height.
pub async fn update_block(height: u64) -> Result<()> {
    println!("Stub: updating block {}", height);
    Ok(())
}
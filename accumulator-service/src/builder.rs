//! Batch builder logic (stubs for now).
use anyhow::Result;

/// Start a fresh build or resume from snapshot.
/// `parquet` path to Parquet file, `resume_from` optional existing snapshot.
pub async fn start_build(parquet: &str, resume_from: Option<&str>) -> Result<()> {
    // TODO: implement batch import from Parquet into MemForest
    println!("Stub: start_build from {} resume {:?}", parquet, resume_from);
    Ok(())
}
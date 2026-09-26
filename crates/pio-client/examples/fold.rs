//! `fold RECORDING.json`: the Rust folds over one recorded stream, printed
//! as JSON. `scripts/fold_parity.py` runs this beside the Python folds.
fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: fold RECORDING.json"))?;
    let recording: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    println!(
        "{}",
        serde_json::to_string(&pio_client::replay::fold(&recording)?)?
    );
    Ok(())
}

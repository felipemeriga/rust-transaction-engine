use anyhow::Context;
use rust_transaction_engine::io::{run_sequential, write_accounts};
use std::fs::File;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .target(env_logger::Target::Stderr)
        .init();
    let path = std::env::args()
        .nth(1)
        .context("usage: cargo run -- <transactions.csv>")?;
    let file = File::open(&path).with_context(|| format!("cannot open {path}"))?;
    let engine = run_sequential(file);
    write_accounts(engine.accounts(), std::io::stdout().lock())?;
    Ok(())
}

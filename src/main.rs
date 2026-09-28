use anyhow::Context;
use rust_transaction_engine::io::write_accounts;
use rust_transaction_engine::runtime::run_sharded;
use std::fs::File;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .target(env_logger::Target::Stderr)
        .init();
    let path = std::env::args()
        .nth(1)
        .context("usage: cargo run -- <transactions.csv>")?;
    let file = File::open(&path).with_context(|| format!("cannot open {path}"))?;
    let engines = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_sharded(file));
    write_accounts(
        engines.iter().flat_map(|e| e.accounts()),
        std::io::stdout().lock(),
    )?;
    Ok(())
}

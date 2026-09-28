//! Mint a big CSV: cargo run --release --example generate -- 10000000 42 > big.csv
fn main() {
    let mut args = std::env::args().skip(1);
    let rows: u32 = args
        .next()
        .and_then(|a| a.parse().ok())
        .unwrap_or(1_000_000);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(42);
    rust_transaction_engine::testgen::generate(rows, seed, std::io::stdout().lock())
        .expect("write to stdout");
}

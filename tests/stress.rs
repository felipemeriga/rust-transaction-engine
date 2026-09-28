use rust_transaction_engine::{io::run_sequential, testgen::generate};

/// Big-file smoke: minted CSV streams through the full pipeline.
/// Run at epoch gates: cargo test --release --test stress -- --ignored
#[test]
#[ignore = "multi-million-row run; execute at epoch gates with --release"]
fn five_million_rows_stream_through() {
    let path = std::env::temp_dir().join("payments_stress.csv");
    let file = std::fs::File::create(&path).unwrap();
    generate(5_000_000, 42, file).unwrap();
    let engine = run_sequential(std::fs::File::open(&path).unwrap());
    assert!(engine.accounts().count() > 0);
    std::fs::remove_file(&path).ok();
}

use rust_transaction_engine::engine::Engine;
use rust_transaction_engine::io::run_sequential;
use rust_transaction_engine::runtime::run_sharded;
use rust_transaction_engine::testgen::generate;
use std::io::Cursor;

/// Sorted formatted account rows from a slice of engines. The sequential
/// side is passed as a one-element slice; the sharded side as the full
/// Vec<Engine> returned by run_sharded.
fn rows(engines: &[Engine]) -> Vec<String> {
    let mut rows: Vec<String> = engines
        .iter()
        .flat_map(|e| e.accounts())
        .map(|(client, a)| {
            format!(
                "{client},{},{},{},{}",
                a.available,
                a.held,
                a.total(),
                a.locked
            )
        })
        .collect();
    rows.sort();
    rows
}

#[tokio::test(flavor = "multi_thread")]
async fn sharded_equals_sequential_on_generated_load() {
    let mut csv = Vec::new();
    generate(200_000, 7, &mut csv).unwrap();

    let sequential = run_sequential(csv.as_slice());
    let sharded = run_sharded(Cursor::new(csv)).await;

    assert_eq!(rows(std::slice::from_ref(&sequential)), rows(&sharded));
}

#[tokio::test(flavor = "multi_thread")]
async fn sharded_handles_all_fixtures() {
    for name in [
        "basic",
        "dispute_resolve",
        "chargeback_lock",
        "withdrawal_dispute",
        "negative_balance",
        "malformed",
        "noise",
        "whitespace",
        "empty",
    ] {
        let bytes = std::fs::read(format!("tests/fixtures/{name}.csv")).unwrap();

        let sequential = run_sequential(bytes.as_slice());
        let sharded = run_sharded(Cursor::new(bytes)).await;

        assert_eq!(
            rows(std::slice::from_ref(&sequential)),
            rows(&sharded),
            "fixture {name}"
        );
    }
}

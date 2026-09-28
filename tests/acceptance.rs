use rust_transaction_engine::io::{run_sequential, write_accounts};

/// Order-insensitive, whitespace-normalized row comparison (spec: row order
/// and spacing don't matter).
fn normalize(csv: &str) -> Vec<String> {
    let mut rows: Vec<String> = csv
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(',').map(str::trim).collect::<Vec<_>>().join(","))
        .collect();
    rows.sort();
    rows
}

fn check(name: &str) {
    let input = std::fs::File::open(format!("tests/fixtures/{name}.csv")).unwrap();
    let engine = run_sequential(input);
    let mut out = Vec::new();
    write_accounts(engine.accounts(), &mut out).unwrap();
    let produced = normalize(std::str::from_utf8(&out).unwrap());
    let expected =
        normalize(&std::fs::read_to_string(format!("tests/fixtures/{name}.expected.csv")).unwrap());
    assert_eq!(produced, expected, "fixture {name}");
}

#[test]
fn basic() {
    check("basic");
}
#[test]
fn dispute_resolve() {
    check("dispute_resolve");
}
#[test]
fn chargeback_lock() {
    check("chargeback_lock");
}
#[test]
fn withdrawal_dispute() {
    check("withdrawal_dispute");
}
#[test]
fn negative_balance() {
    check("negative_balance");
}
#[test]
fn malformed() {
    check("malformed");
}
#[test]
fn noise() {
    check("noise");
}
#[test]
fn whitespace() {
    check("whitespace");
}
#[test]
fn empty() {
    check("empty");
}

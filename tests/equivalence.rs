use rust_transaction_engine::io::run_sequential;
use rust_transaction_engine::runtime::run_sharded;
use rust_transaction_engine::testgen::generate;
use std::io::Cursor;

fn assert_engines_equivalent(
    seq: &rust_transaction_engine::engine::Engine,
    shard: &rust_transaction_engine::engine::Engine,
) {
    let seq_accounts: std::collections::BTreeMap<u16, _> = seq
        .accounts()
        .map(|(c, a)| (c, (a.available, a.held, a.locked)))
        .collect();
    let shard_accounts: std::collections::BTreeMap<u16, _> = shard
        .accounts()
        .map(|(c, a)| (c, (a.available, a.held, a.locked)))
        .collect();
    assert_eq!(
        seq_accounts, shard_accounts,
        "Sequential and sharded engines must produce identical account states"
    );
}

#[test]
fn equivalence_on_all_fixtures() {
    let fixtures = vec![
        (
            "single_client_deposits",
            "\
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 1, 2, 2.0
deposit, 1, 3, 3.0
",
        ),
        (
            "multi_client_interleaved",
            "\
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 2, 2, 2.0
deposit, 1, 3, 3.0
deposit, 2, 4, 4.0
",
        ),
        (
            "disputes_and_chargebacks",
            "\
type, client, tx, amount
deposit, 1, 1, 10.0
deposit, 2, 2, 20.0
dispute, 1, 1,
resolve, 1, 1,
dispute, 2, 2,
chargeback, 2, 2,
",
        ),
        (
            "withdrawals_and_disputes",
            "\
type, client, tx, amount
deposit, 1, 1, 50.0
withdrawal, 1, 2, 10.0
dispute, 1, 2,
deposit, 2, 3, 100.0
withdrawal, 2, 4, 30.0
dispute, 2, 4,
chargeback, 2, 4,
",
        ),
    ];

    for (_name, data) in fixtures {
        let seq = run_sequential(data.as_bytes());
        let data_owned = data.to_string().into_bytes();
        let shard = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_sharded(Cursor::new(data_owned)))
            .unwrap();
        assert_engines_equivalent(&seq, &shard);
    }
}

#[test]
fn equivalence_on_generated_load() {
    let mut data = Vec::new();
    generate(200_000, 42, &mut data).unwrap();

    let seq = run_sequential(data.as_slice());
    let shard = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_sharded(Cursor::new(data)))
        .unwrap();
    assert_engines_equivalent(&seq, &shard);
}

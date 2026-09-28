use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rust-transaction-engine"))
}

#[test]
fn processes_file_and_prints_csv_to_stdout() {
    let dir = std::env::temp_dir();
    let input = dir.join("cli_test_input.csv");
    std::fs::write(
        &input,
        "type, client, tx, amount\ndeposit, 1, 1, 3.0\nwithdrawal, 1, 2, 1.0\n",
    )
    .unwrap();
    let out = bin().arg(&input).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let mut lines = stdout.lines();
    assert_eq!(lines.next().unwrap(), "client,available,held,total,locked");
    assert_eq!(lines.next().unwrap(), "1,2.0000,0.0000,2.0000,false");
    assert_eq!(lines.next(), None);
    std::fs::remove_file(&input).ok();
}

#[test]
fn missing_argument_fails_with_no_stdout() {
    let out = bin().output().unwrap();
    assert!(!out.status.success());
    assert!(
        out.stdout.is_empty(),
        "stdout must stay pure even on failure"
    );
}

#[test]
fn unreadable_file_fails_with_no_stdout() {
    let out = bin().arg("does_not_exist.csv").output().unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}

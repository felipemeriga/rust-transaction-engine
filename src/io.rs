use crate::{
    account::Account,
    engine::Engine,
    transaction::{RawRecord, Transaction},
};
use std::io::{Read, Write};

/// Streams rows, converting each into a Transaction. Layer-2 failures
/// (structural or field errors) are logged at ERROR and skipped — the
/// stream never stops for a typo.
pub fn read_transactions(input: impl Read) -> impl Iterator<Item = Transaction> {
    csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input)
        .into_deserialize::<RawRecord>()
        .enumerate()
        .filter_map(|(i, row)| {
            let line = i + 2; // line 1 is the header
            match row.map_err(|e| crate::error::RowError::MalformedRow(e.to_string())) {
                Ok(raw) => match Transaction::try_from(raw) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        log::error!("line {line}: {e}");
                        None
                    }
                },
                Err(e) => {
                    log::error!("line {line}: {e}");
                    None
                }
            }
        })
}

/// Writes the final account states as CSV: header + one row per client,
/// four decimal places, row order unspecified.
pub fn write_accounts<'a>(
    accounts: impl Iterator<Item = (u16, &'a Account)>,
    out: impl Write,
) -> csv::Result<()> {
    let mut w = csv::Writer::from_writer(out);
    w.write_record(["client", "available", "held", "total", "locked"])?;
    for (client, a) in accounts {
        w.write_record([
            client.to_string(),
            a.available.to_string(),
            a.held.to_string(),
            a.total().to_string(),
            a.locked.to_string(),
        ])?;
    }
    w.flush()?;
    Ok(())
}

/// The reference runtime: one engine, rows in file order.
/// Layer-3 rejections are logged at WARN and ignored.
pub fn run_sequential(input: impl Read) -> Engine {
    let mut engine = Engine::new();
    for t in read_transactions(input) {
        if let Err(rejection) = engine.process(t) {
            log::warn!("{rejection}");
        }
    }
    engine
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_skips_malformed_rows() {
        let data = "\
type, client, tx, amount
deposit, 1, 1, 2.0
deposit, abc, 2, 1.0
teleport, 1, 3, 1.0
deposit, 1, 4, 1.00005
dispute, 1, 1,
dispute, 1, 1
";
        let txs: Vec<Transaction> = read_transactions(data.as_bytes()).collect();
        // 1 valid deposit + 2 valid disputes (with and without trailing comma)
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[1], Transaction::Dispute { client: 1, tx: 1 });
        assert_eq!(txs[2], Transaction::Dispute { client: 1, tx: 1 });
    }

    #[test]
    fn empty_input() {
        assert_eq!(
            read_transactions("type, client, tx, amount\n".as_bytes()).count(),
            0
        );
        assert_eq!(read_transactions("".as_bytes()).count(), 0);
        let engine = run_sequential("".as_bytes());
        let mut out = Vec::new();
        write_accounts(engine.accounts(), &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "client,available,held,total,locked\n"
        );
    }

    #[test]
    fn end_to_end_sequential() {
        let data = "\
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 2, 2, 2.0
deposit, 1, 3, 2.0
withdrawal, 1, 4, 1.5
withdrawal, 2, 5, 3.0
";
        let engine = run_sequential(data.as_bytes());
        let mut out = Vec::new();
        write_accounts(engine.accounts(), &mut out).unwrap();
        let mut rows: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().skip(1).collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                "1,1.5000,0.0000,1.5000,false",
                "2,2.0000,0.0000,2.0000,false"
            ]
        );
    }
}

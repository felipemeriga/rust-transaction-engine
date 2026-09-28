use crate::{amount::Amount, error::RowError};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RawRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub client: String,
    pub tx: String,
    #[serde(default)]
    pub amount: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transaction {
    Deposit {
        client: u16,
        tx: u32,
        amount: Amount,
    },
    Withdrawal {
        client: u16,
        tx: u32,
        amount: Amount,
    },
    Dispute {
        client: u16,
        tx: u32,
    },
    Resolve {
        client: u16,
        tx: u32,
    },
    Chargeback {
        client: u16,
        tx: u32,
    },
}

impl Transaction {
    pub fn client(&self) -> u16 {
        match *self {
            Transaction::Deposit { client, .. }
            | Transaction::Withdrawal { client, .. }
            | Transaction::Dispute { client, .. }
            | Transaction::Resolve { client, .. }
            | Transaction::Chargeback { client, .. } => client,
        }
    }
}

impl TryFrom<RawRecord> for Transaction {
    type Error = RowError;

    fn try_from(r: RawRecord) -> Result<Self, RowError> {
        let client: u16 = r
            .client
            .trim()
            .parse()
            .map_err(|_| RowError::InvalidClientId(r.client.clone()))?;
        let tx: u32 =
            r.tx.trim()
                .parse()
                .map_err(|_| RowError::InvalidTxId(r.tx.clone()))?;
        let amount = r.amount.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let movement_amount = || -> Result<Amount, RowError> {
            let s = amount.ok_or(RowError::MissingAmount)?;
            let parsed: Amount = s.parse()?;
            if !parsed.is_positive() {
                return Err(RowError::NonPositiveAmount(s.into()));
            }
            Ok(parsed)
        };
        // Verdict rows carry no amount; a present value is silently tolerated.
        match r.kind.trim() {
            "deposit" => Ok(Transaction::Deposit {
                client,
                tx,
                amount: movement_amount()?,
            }),
            "withdrawal" => Ok(Transaction::Withdrawal {
                client,
                tx,
                amount: movement_amount()?,
            }),
            "dispute" => Ok(Transaction::Dispute { client, tx }),
            "resolve" => Ok(Transaction::Resolve { client, tx }),
            "chargeback" => Ok(Transaction::Chargeback { client, tx }),
            other => Err(RowError::UnknownTransactionType(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(kind: &str, client: &str, tx: &str, amount: Option<&str>) -> RawRecord {
        RawRecord {
            kind: kind.into(),
            client: client.into(),
            tx: tx.into(),
            amount: amount.map(String::from),
        }
    }

    #[test]
    fn valid_movements_and_verdicts() {
        let dep = Transaction::try_from(raw("deposit", "1", "7", Some("1.5"))).unwrap();
        assert_eq!(
            dep,
            Transaction::Deposit {
                client: 1,
                tx: 7,
                amount: "1.5".parse().unwrap()
            }
        );
        let wd = Transaction::try_from(raw("withdrawal", "2", "8", Some("0.0001"))).unwrap();
        assert_eq!(
            wd,
            Transaction::Withdrawal {
                client: 2,
                tx: 8,
                amount: "0.0001".parse().unwrap()
            }
        );
        for (kind, want) in [
            ("dispute", Transaction::Dispute { client: 3, tx: 9 }),
            ("resolve", Transaction::Resolve { client: 3, tx: 9 }),
            ("chargeback", Transaction::Chargeback { client: 3, tx: 9 }),
        ] {
            assert_eq!(
                Transaction::try_from(raw(kind, "3", "9", None)).unwrap(),
                want
            );
        }
    }

    #[test]
    fn verdict_with_superfluous_amount_is_tolerated() {
        let t = Transaction::try_from(raw("dispute", "1", "7", Some("9.99"))).unwrap();
        assert_eq!(t, Transaction::Dispute { client: 1, tx: 7 });
    }

    #[test]
    fn client_accessor() {
        assert_eq!(
            Transaction::try_from(raw("resolve", "42", "1", None))
                .unwrap()
                .client(),
            42
        );
    }

    #[test]
    fn rejects_bad_rows() {
        assert_eq!(
            Transaction::try_from(raw("deposit", "70000", "1", Some("1"))).unwrap_err(),
            RowError::InvalidClientId("70000".into())
        );
        assert_eq!(
            Transaction::try_from(raw("deposit", "1", "-1", Some("1"))).unwrap_err(),
            RowError::InvalidTxId("-1".into())
        );
        assert_eq!(
            Transaction::try_from(raw("teleport", "1", "1", None)).unwrap_err(),
            RowError::UnknownTransactionType("teleport".into())
        );
        assert_eq!(
            Transaction::try_from(raw("deposit", "1", "1", None)).unwrap_err(),
            RowError::MissingAmount
        );
        assert_eq!(
            Transaction::try_from(raw("deposit", "1", "1", Some(""))).unwrap_err(),
            RowError::MissingAmount
        );
        assert_eq!(
            Transaction::try_from(raw("deposit", "1", "1", Some("-2"))).unwrap_err(),
            RowError::NonPositiveAmount("-2".into())
        );
        assert_eq!(
            Transaction::try_from(raw("withdrawal", "1", "1", Some("0.0"))).unwrap_err(),
            RowError::NonPositiveAmount("0.0".into())
        );
        assert_eq!(
            Transaction::try_from(raw("deposit", "1", "1", Some("1.00005"))).unwrap_err(),
            RowError::InvalidAmount("1.00005".into())
        );
    }
}

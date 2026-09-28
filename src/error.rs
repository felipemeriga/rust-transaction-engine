use thiserror::Error;

/// Layer 2 — the row itself is broken: log ERROR, skip the row.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RowError {
    #[error("malformed row: {0}")]
    MalformedRow(String),
    #[error("invalid client id: {0:?}")]
    InvalidClientId(String),
    #[error("invalid tx id: {0:?}")]
    InvalidTxId(String),
    #[error("unknown transaction type: {0:?}")]
    UnknownTransactionType(String),
    #[error("invalid amount: {0:?}")]
    InvalidAmount(String),
    #[error("missing amount")]
    MissingAmount,
    #[error("non-positive amount: {0:?}")]
    NonPositiveAmount(String),
}

/// Layer 3 — the row is valid but the ledger rules say no: log WARN, ignore.
#[derive(Debug, Error, PartialEq, Eq, Clone, Copy)]
pub enum Rejection {
    #[error("tx {0} does not exist")]
    UnknownTransaction(u32),
    #[error("client {client} does not own tx {tx}")]
    ClientMismatch { tx: u32, client: u16 },
    #[error("tx {0} is already under dispute")]
    AlreadyDisputed(u32),
    #[error("tx {0} is not under dispute")]
    NotUnderDispute(u32),
    #[error("tx {0} was charged back")]
    TransactionChargedBack(u32),
    #[error("account {0} is locked")]
    AccountLocked(u16),
    #[error("insufficient available funds for tx {0}")]
    InsufficientFunds(u32),
    #[error("duplicate tx id {0}")]
    DuplicateTransactionId(u32),
    #[error("balance overflow applying tx {0}")]
    Overflow(u32),
}

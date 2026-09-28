use crate::{account::Account, amount::Amount, error::Rejection, transaction::Transaction};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisputeState {
    Undisputed,
    Disputed,
    ChargedBack,
}

#[derive(Debug)]
struct TxRecord {
    client: u16,
    amount: Amount,
    state: DisputeState,
}

#[derive(Debug, Default)]
pub struct Engine {
    accounts: HashMap<u16, Account>,
    txs: HashMap<u32, TxRecord>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accounts(&self) -> impl Iterator<Item = (u16, &Account)> {
        self.accounts.iter().map(|(c, a)| (*c, a))
    }

    pub fn process(&mut self, t: Transaction) -> Result<(), Rejection> {
        match t {
            Transaction::Deposit { client, tx, amount } => {
                let account = self.movement_checks(client, tx)?;
                account.available = account
                    .available
                    .checked_add(amount)
                    .ok_or(Rejection::Overflow(tx))?;
                self.txs.insert(
                    tx,
                    TxRecord {
                        client,
                        amount,
                        state: DisputeState::Undisputed,
                    },
                );
                Ok(())
            }
            Transaction::Withdrawal { client, tx, amount } => {
                let account = self.movement_checks(client, tx)?;
                if account.available < amount {
                    return Err(Rejection::InsufficientFunds(tx));
                }
                account.available = account
                    .available
                    .checked_sub(amount)
                    .ok_or(Rejection::Overflow(tx))?;
                self.txs.insert(
                    tx,
                    TxRecord {
                        client,
                        amount,
                        state: DisputeState::Undisputed,
                    },
                );
                Ok(())
            }
            _ => unimplemented!("verdicts land in the next commit"),
        }
    }

    /// Shared gate for client-initiated movements. Creates the account if new
    /// (spec: unknown clients get a record), then rejects on duplicates/locks.
    fn movement_checks(&mut self, client: u16, tx: u32) -> Result<&mut Account, Rejection> {
        let account = self.accounts.entry(client).or_default();
        if account.locked {
            return Err(Rejection::AccountLocked(client));
        }
        if self.txs.contains_key(&tx) {
            return Err(Rejection::DuplicateTransactionId(tx));
        }
        Ok(account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn amt(s: &str) -> Amount {
        s.parse().unwrap()
    }

    pub(crate) fn deposit(
        e: &mut Engine,
        client: u16,
        tx: u32,
        amount: &str,
    ) -> Result<(), Rejection> {
        e.process(Transaction::Deposit {
            client,
            tx,
            amount: amt(amount),
        })
    }

    pub(crate) fn withdraw(
        e: &mut Engine,
        client: u16,
        tx: u32,
        amount: &str,
    ) -> Result<(), Rejection> {
        e.process(Transaction::Withdrawal {
            client,
            tx,
            amount: amt(amount),
        })
    }

    pub(crate) fn account(e: &Engine, client: u16) -> Account {
        e.accounts()
            .find(|(c, _)| *c == client)
            .map(|(_, a)| *a)
            .unwrap()
    }

    #[test]
    fn deposit_credits_available() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "1.5").unwrap();
        deposit(&mut e, 1, 2, "2.5").unwrap();
        let a = account(&e, 1);
        assert_eq!(
            (a.available, a.held, a.total(), a.locked),
            (amt("4"), Amount::ZERO, amt("4"), false)
        );
    }

    #[test]
    fn withdrawal_debits_available() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "5").unwrap();
        withdraw(&mut e, 1, 2, "1.5").unwrap();
        assert_eq!(account(&e, 1).available, amt("3.5"));
    }

    #[test]
    fn withdrawal_of_entire_balance_succeeds() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "5").unwrap();
        withdraw(&mut e, 1, 2, "5").unwrap();
        assert_eq!(account(&e, 1).available, Amount::ZERO);
    }

    #[test]
    fn insufficient_funds_rejects_and_changes_nothing() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "1").unwrap();
        assert_eq!(
            withdraw(&mut e, 1, 2, "1.0001"),
            Err(Rejection::InsufficientFunds(2))
        );
        assert_eq!(account(&e, 1).available, amt("1"));
    }

    #[test]
    fn failed_withdrawal_creates_account() {
        // Spec: "if a client doesn't exist, create a new record" — even when the movement fails.
        let mut e = Engine::new();
        assert_eq!(
            withdraw(&mut e, 9, 1, "1"),
            Err(Rejection::InsufficientFunds(1))
        );
        assert_eq!(account(&e, 9), Account::default());
    }

    #[test]
    fn duplicate_tx_id_rejected() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "1").unwrap();
        assert_eq!(
            deposit(&mut e, 1, 1, "1"),
            Err(Rejection::DuplicateTransactionId(1))
        );
        assert_eq!(
            withdraw(&mut e, 2, 1, "1"),
            Err(Rejection::DuplicateTransactionId(1))
        );
        assert_eq!(account(&e, 1).available, amt("1"));
    }
}

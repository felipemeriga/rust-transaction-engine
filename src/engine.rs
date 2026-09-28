use crate::{account::Account, amount::Amount, error::Rejection, transaction::Transaction};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisputeState {
    Undisputed,
    Disputed,
    ChargedBack,
}

/// Deliberately does not record whether the movement was a deposit or a
/// withdrawal: disputes apply the same hold formula to both, so the engine
/// never consults the direction. A system needing type-aware dispute math
/// or a historical audit trail would add it here.
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

    /// Merge an account from another shard into this engine.
    /// Used to combine engines from sharded workers.
    pub fn merge_account(&mut self, client: u16, account: &Account) {
        self.accounts.insert(client, *account);
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
            Transaction::Dispute { client, tx } => {
                let (record, account) = self.referenced(client, tx)?;
                match record.state {
                    DisputeState::Disputed => Err(Rejection::AlreadyDisputed(tx)),
                    DisputeState::ChargedBack => Err(Rejection::TransactionChargedBack(tx)),
                    DisputeState::Undisputed => {
                        let new_available = account
                            .available
                            .checked_sub(record.amount)
                            .ok_or(Rejection::Overflow(tx))?;
                        let new_held = account
                            .held
                            .checked_add(record.amount)
                            .ok_or(Rejection::Overflow(tx))?;
                        account.available = new_available;
                        account.held = new_held;
                        record.state = DisputeState::Disputed;
                        Ok(())
                    }
                }
            }
            Transaction::Resolve { client, tx } => {
                let (record, account) = self.referenced(client, tx)?;
                match record.state {
                    DisputeState::Undisputed => Err(Rejection::NotUnderDispute(tx)),
                    DisputeState::ChargedBack => Err(Rejection::TransactionChargedBack(tx)),
                    DisputeState::Disputed => {
                        let new_held = account
                            .held
                            .checked_sub(record.amount)
                            .ok_or(Rejection::Overflow(tx))?;
                        let new_available = account
                            .available
                            .checked_add(record.amount)
                            .ok_or(Rejection::Overflow(tx))?;
                        account.held = new_held;
                        account.available = new_available;
                        record.state = DisputeState::Undisputed;
                        Ok(())
                    }
                }
            }
            Transaction::Chargeback { client, tx } => {
                let (record, account) = self.referenced(client, tx)?;
                match record.state {
                    DisputeState::Undisputed => Err(Rejection::NotUnderDispute(tx)),
                    DisputeState::ChargedBack => Err(Rejection::TransactionChargedBack(tx)),
                    DisputeState::Disputed => {
                        account.held = account
                            .held
                            .checked_sub(record.amount)
                            .ok_or(Rejection::Overflow(tx))?;
                        account.locked = true;
                        record.state = DisputeState::ChargedBack;
                        Ok(())
                    }
                }
            }
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

    /// Shared gate for partner verdicts: the referenced tx must exist and belong
    /// to the row's client. Locked accounts still process verdicts (premise 4).
    fn referenced(
        &mut self,
        client: u16,
        tx: u32,
    ) -> Result<(&mut TxRecord, &mut Account), Rejection> {
        let record = self
            .txs
            .get_mut(&tx)
            .ok_or(Rejection::UnknownTransaction(tx))?;
        if record.client != client {
            return Err(Rejection::ClientMismatch { tx, client });
        }
        let account = self
            .accounts
            .get_mut(&record.client)
            .expect("invariant: storing a tx always created its account first");
        Ok((record, account))
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

    pub(crate) fn verdict(
        e: &mut Engine,
        kind: &str,
        client: u16,
        tx: u32,
    ) -> Result<(), Rejection> {
        e.process(match kind {
            "dispute" => Transaction::Dispute { client, tx },
            "resolve" => Transaction::Resolve { client, tx },
            _ => Transaction::Chargeback { client, tx },
        })
    }

    #[test]
    fn dispute_holds_funds() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        let a = account(&e, 1);
        assert_eq!(
            (a.available, a.held, a.total()),
            (Amount::ZERO, amt("10"), amt("10"))
        );
    }

    #[test]
    fn resolve_is_a_perfect_undo_and_reopens() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        verdict(&mut e, "resolve", 1, 1).unwrap();
        let a = account(&e, 1);
        assert_eq!(
            (a.available, a.held, a.locked),
            (amt("10"), Amount::ZERO, false)
        );
        // resolve returns the tx to Undisputed: it can be disputed again
        verdict(&mut e, "dispute", 1, 1).unwrap();
        assert_eq!(account(&e, 1).held, amt("10"));
    }

    #[test]
    fn chargeback_removes_funds_and_locks() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        verdict(&mut e, "chargeback", 1, 1).unwrap();
        let a = account(&e, 1);
        assert_eq!(
            (a.available, a.held, a.total(), a.locked),
            (Amount::ZERO, Amount::ZERO, Amount::ZERO, true)
        );
    }

    #[test]
    fn fraud_scenario_ends_negative_and_locked() {
        // deposit 10 → withdraw 10 → dispute the deposit → chargeback
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        withdraw(&mut e, 1, 2, "10").unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        let mid = account(&e, 1);
        assert_eq!(
            (mid.available, mid.held, mid.total()),
            (amt("-10"), amt("10"), Amount::ZERO)
        );
        verdict(&mut e, "chargeback", 1, 1).unwrap();
        let a = account(&e, 1);
        assert_eq!(
            (a.available, a.total(), a.locked),
            (amt("-10"), amt("-10"), true)
        );
    }

    #[test]
    fn withdrawal_dispute_uses_literal_spec_math() {
        // premise 1: both movement types disputable, same formula
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        withdraw(&mut e, 1, 2, "4").unwrap();
        verdict(&mut e, "dispute", 1, 2).unwrap();
        let a = account(&e, 1);
        assert_eq!(
            (a.available, a.held, a.total()),
            (amt("2"), amt("4"), amt("6"))
        );
    }

    #[test]
    fn reopened_dispute_can_charge_back() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        verdict(&mut e, "resolve", 1, 1).unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        verdict(&mut e, "chargeback", 1, 1).unwrap();
        let a = account(&e, 1);
        assert_eq!((a.total(), a.locked), (Amount::ZERO, true));
    }

    #[test]
    fn locked_account_rejects_movements_but_processes_verdicts() {
        // premise 4
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        deposit(&mut e, 1, 2, "5").unwrap();
        verdict(&mut e, "dispute", 1, 1).unwrap();
        verdict(&mut e, "chargeback", 1, 1).unwrap();
        assert_eq!(
            deposit(&mut e, 1, 3, "99"),
            Err(Rejection::AccountLocked(1))
        );
        assert_eq!(
            withdraw(&mut e, 1, 4, "1"),
            Err(Rejection::AccountLocked(1))
        );
        verdict(&mut e, "dispute", 1, 2).unwrap();
        verdict(&mut e, "chargeback", 1, 2).unwrap();
        let a = account(&e, 1);
        assert_eq!((a.total(), a.locked), (Amount::ZERO, true));
    }

    #[test]
    fn verdict_rejections() {
        let mut e = Engine::new();
        deposit(&mut e, 1, 1, "10").unwrap();
        assert_eq!(
            verdict(&mut e, "dispute", 1, 99),
            Err(Rejection::UnknownTransaction(99))
        );
        assert_eq!(
            verdict(&mut e, "dispute", 2, 1),
            Err(Rejection::ClientMismatch { tx: 1, client: 2 })
        );
        assert_eq!(
            verdict(&mut e, "resolve", 1, 1),
            Err(Rejection::NotUnderDispute(1))
        );
        assert_eq!(
            verdict(&mut e, "chargeback", 1, 1),
            Err(Rejection::NotUnderDispute(1))
        );
        verdict(&mut e, "dispute", 1, 1).unwrap();
        assert_eq!(
            verdict(&mut e, "dispute", 1, 1),
            Err(Rejection::AlreadyDisputed(1))
        );
        verdict(&mut e, "chargeback", 1, 1).unwrap();
        for kind in ["dispute", "resolve", "chargeback"] {
            assert_eq!(
                verdict(&mut e, kind, 1, 1),
                Err(Rejection::TransactionChargedBack(1))
            );
        }
    }

    #[test]
    fn verdict_never_creates_account() {
        let mut e = Engine::new();
        assert_eq!(
            verdict(&mut e, "dispute", 5, 1),
            Err(Rejection::UnknownTransaction(1))
        );
        assert_eq!(e.accounts().count(), 0);
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

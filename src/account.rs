use crate::amount::Amount;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Account {
    pub available: Amount,
    pub held: Amount,
    pub locked: bool,
}

impl Account {
    /// total = available + held, by construction (spec §3.2).
    pub fn total(&self) -> Amount {
        Amount::from_raw(self.available.raw().saturating_add(self.held.raw()))
    }
}

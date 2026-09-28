use crate::error::RowError;
use std::{fmt, str::FromStr};

pub const SCALE: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Amount(i64);

impl Amount {
    pub const ZERO: Amount = Amount(0);

    pub fn from_raw(units: i64) -> Amount {
        Amount(units)
    }

    pub fn raw(self) -> i64 {
        self.0
    }

    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub fn checked_add(self, other: Amount) -> Option<Amount> {
        self.0.checked_add(other.0).map(Amount)
    }

    pub fn checked_sub(self, other: Amount) -> Option<Amount> {
        self.0.checked_sub(other.0).map(Amount)
    }
}

impl FromStr for Amount {
    type Err = RowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || RowError::InvalidAmount(s.into());
        let trimmed = s.trim();
        let (negative, digits) = match trimmed.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
        if int.is_empty() && frac.is_empty()
            || frac.len() > 4
            || !int.chars().all(|c| c.is_ascii_digit())
            || !frac.chars().all(|c| c.is_ascii_digit())
        {
            return Err(err());
        }
        let int: i64 = if int.is_empty() {
            0
        } else {
            int.parse().map_err(|_| err())?
        };
        let frac: i64 = format!("{frac:0<4}").parse().unwrap_or(0);
        let units = int
            .checked_mul(SCALE)
            .and_then(|v| v.checked_add(frac))
            .ok_or_else(err)?;
        Ok(Amount(if negative { -units } else { units }))
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:04}", abs / SCALE as u64, abs % SCALE as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amt(s: &str) -> Amount {
        s.parse().unwrap()
    }

    #[test]
    fn parse_valid() {
        assert_eq!(amt("1"), Amount::from_raw(10_000));
        assert_eq!(amt("1.5"), Amount::from_raw(15_000));
        assert_eq!(amt(" 1.5 "), Amount::from_raw(15_000));
        assert_eq!(amt("0.0001"), Amount::from_raw(1));
        assert_eq!(amt("1.2345"), Amount::from_raw(12_345));
        assert_eq!(amt("-5.0"), Amount::from_raw(-50_000));
        assert_eq!(amt(".5"), Amount::from_raw(5_000));
        assert_eq!(amt("5."), Amount::from_raw(50_000));
    }

    #[test]
    fn parse_invalid() {
        for bad in [
            "",
            " ",
            ".",
            "-",
            "abc",
            "1.00005",
            "1.2.3",
            "1,5",
            "1e3",
            "99999999999999999999",
            "9999999999999999.0",
        ] {
            assert!(bad.parse::<Amount>().is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn display_four_decimals() {
        assert_eq!(amt("1.5").to_string(), "1.5000");
        assert_eq!(amt("0").to_string(), "0.0000");
        assert_eq!(amt("-10").to_string(), "-10.0000");
        assert_eq!(amt("-0.0001").to_string(), "-0.0001");
    }

    #[test]
    fn checked_ops() {
        assert_eq!(amt("1.5").checked_add(amt("2.5")), Some(amt("4")));
        assert_eq!(amt("1").checked_sub(amt("2.5")), Some(amt("-1.5")));
        assert_eq!(
            Amount::from_raw(i64::MAX).checked_add(Amount::from_raw(1)),
            None
        );
        assert!(amt("0.0001").is_positive());
        assert!(!Amount::ZERO.is_positive());
        assert!(!amt("-1").is_positive());
    }
}

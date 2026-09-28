use std::io::{self, Write};

/// Deterministic pseudo-random transaction stream: ~55% deposits, ~25%
/// withdrawals, ~18% verdicts against real earlier movements, ~2% noise
/// referencing unknown tx ids. No `rand` dependency: a fixed-constant LCG
/// keeps the byte stream reproducible for a given seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

pub fn generate(rows: u32, seed: u64, out: impl Write) -> io::Result<()> {
    let mut rng = Lcg(seed);
    let mut out = io::BufWriter::new(out);
    writeln!(out, "type, client, tx, amount")?;
    let mut movements: Vec<(u32, u16)> = Vec::new(); // (tx, client)
    for tx in 1..=rows {
        let client = (rng.next() % 200) as u16 + 1;
        let roll = rng.next() % 100;
        match roll {
            0..=54 => {
                let units = rng.next() % 1_000_000 + 1; // 0.0001 ..= 100.0000
                writeln!(
                    out,
                    "deposit, {client}, {tx}, {}.{:04}",
                    units / 10_000,
                    units % 10_000
                )?;
                movements.push((tx, client));
            }
            55..=79 => {
                let units = rng.next() % 500_000 + 1;
                writeln!(
                    out,
                    "withdrawal, {client}, {tx}, {}.{:04}",
                    units / 10_000,
                    units % 10_000
                )?;
                movements.push((tx, client));
            }
            80..=97 if !movements.is_empty() => {
                let (target, owner) = movements[rng.next() as usize % movements.len()];
                let kind = match roll {
                    80..=89 => "dispute",
                    90..=94 => "resolve",
                    _ => "chargeback",
                };
                writeln!(out, "{kind}, {owner}, {target},")?;
            }
            _ => writeln!(out, "dispute, {client}, {},", u32::MAX - tx)?, // unknown tx noise
        }
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::run_sequential;
    use std::io::BufRead;

    #[test]
    fn deterministic_for_same_seed() {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        generate(1_000, 42, &mut a).unwrap();
        generate(1_000, 42, &mut b).unwrap();
        assert_eq!(a, b);
        let mut c = Vec::new();
        generate(1_000, 43, &mut c).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn output_is_processable_and_exercises_disputes() {
        let mut data = Vec::new();
        generate(50_000, 7, &mut data).unwrap();
        assert_eq!(data.as_slice().lines().count(), 50_001); // header + rows
        let engine = run_sequential(data.as_slice());
        let locked = engine.accounts().filter(|(_, a)| a.locked).count();
        let held = engine
            .accounts()
            .filter(|(_, a)| a.held != crate::amount::Amount::ZERO)
            .count();
        assert!(locked > 0, "chargebacks must occur");
        assert!(held > 0, "open disputes must remain");
    }
}

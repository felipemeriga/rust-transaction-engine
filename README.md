# rust-transaction-engine

This is a transaction engine meant to validate a series of transactions coming from a CSV file.

## The Approach

The approach for solving this exercise, since AI was used from ideation until the last steps, was to use AI as a pair-programming partner that helped in many different steps of the whole resolution.

Many developers are integrating AI into their workflows, using different skills, plugins, orchestrating agents, and RAG workflows, which I also use in my daily work. But for this exercise, I considered that a one-shot session with Claude Code is sufficient, without the need for a RAG workflow or multi-agents, which would be overkill.

### The steps

I thought about dividing this problem into steps, so we could attack them with the proper questions and tests.

I divided the project into four steps, which is what I usually do in my daily work:

- **Ideation:** A session with AI, only discussing the requirements that were given, the gaps and edge cases that the document doesn't state, what we can infer, what assumptions we can make, what makes sense in the context of this solution, and other discussions about the problem and the product itself.
- **Planning:** Defining the basic rules of the system, which libraries we should use, the testing strategy, and so on.
- **Spec design:** Designing the spec of the whole system.
- **Execution in epochs:** Executing each part of the spec, stopping and testing it, and making sure we can move to the next part.

---

## What This Is

A streaming payment-ledger engine that reads a CSV of client transactions, applies deposit, withdrawal, dispute, resolve, and chargeback operations in order, and emits final account states. The library core (`Engine`) is completely free of I/O: it owns only a hash-map of accounts and a hash-map of stored transaction records, and exposes a single `process` method. The CLI is a thin wrapper that wires a file reader, a sharded async runtime, and a CSV writer around that pure core. The design prioritises correctness by construction — no shared mutable state across threads, no floating-point arithmetic anywhere in the ledger path, and a layered error model that prevents a single malformed row from stopping the stream.

---

## Usage

```bash
# Process a CSV file; final account states go to stdout.
cargo run -- transactions.csv > accounts.csv

# Logs (parse errors, rejected transactions) go to stderr via RUST_LOG.
RUST_LOG=info cargo run -- transactions.csv > accounts.csv
# Default filter is warn; levels: error | warn | info | debug | trace

# Generate a large synthetic CSV for benchmarking (10 million rows, seed 42).
cargo run --release --example generate -- 10000000 42 > big.csv
cargo run --release -- big.csv > /dev/null
```

The output CSV has the header `client,available,held,total,locked`; row order is unspecified. All amounts are formatted to four decimal places.

---

## Assumptions

The specification leaves several behaviours underspecified. The following premises were established during the ideation phase and are encoded in the implementation and tests.

**1. Disputes apply to both deposits and withdrawals using the literal spec formula.**
The specification describes the dispute hold formula in deposit-shaped terms: `available -= amount`, `held += amount`. This implementation applies that same formula to withdrawal disputes as well, rather than guessing at an inverted type-aware alternative. Concretely, disputing a withdrawal of 4.0 on a 10.0 balance results in `available = 10 - 4 - 4 = 2`, `held = 4`, `total = 6` — the amount is moved from available to held regardless of direction. A real product would likely use type-aware math (e.g. reversing the withdrawal credit for the hold), but implementing that here would require storing the movement type in `TxRecord`, which was deliberately omitted to reflect the specification as written.

**2. Negative balances are legal and represent a recorded loss.**
The classic fraud pattern — deposit, withdraw the full balance, then dispute the original deposit — drives the available balance negative during the dispute phase (available becomes `−amount` while held holds it). When the chargeback follows, held is zeroed and the account ends with a negative total equal to the withdrawn amount. This is the mathematically correct representation of what happened: the money left and the dispute confirmed it. The account is locked at this point, preventing further debits.

**3. Locked accounts refuse new deposits and withdrawals but continue to process disputes, resolves, and chargebacks.**
Locking an account freezes the client, not any open investigations. If a client has two deposits in flight when a chargeback on the first locks them, the partner system must still be able to resolve or charge back the second dispute. Blocking verdict processing after a lock would silently corrupt the held balance.

**4. A resolve returns the transaction to Undisputed; it is re-disputable.**
Resolve is a perfect undo: it moves the amount back from held to available and resets the transaction's state to `Undisputed`. This means a resolved transaction can be disputed a second time, and can ultimately be charged back on the second dispute. Chargeback, by contrast, is terminal: no further dispute, resolve, or chargeback is accepted on a charged-back transaction.

**5. Verdicts on unknown transactions, wrong-client references, or wrong state are ignored as partner noise.**
If a dispute arrives for a transaction ID that does not exist, or for a transaction owned by a different client, or for a transaction that is already disputed or already charged back, the row is logged at `WARN` and skipped. No state changes. The same applies to resolves and chargebacks on non-disputed transactions. This matches real partner-network behaviour where out-of-order or duplicated messages are expected and must not corrupt the ledger.

**6. Malformed rows are logged at ERROR and skipped; the stream never stops.**
A row with an unparseable client ID, an unknown transaction type, a missing amount on a movement, or an amount with more than four decimal places is a Layer-2 (`RowError`) failure. It is logged at `ERROR` and the iterator simply moves to the next row. Amounts with more than four decimal places are rejected outright rather than silently rounded, because rounding introduces money from nowhere or destroys it.

**7. Superfluous amount fields on verdict rows are silently tolerated.**
The specification says dispute/resolve/chargeback rows have no amount column. In practice, CSV generators sometimes emit a blank or populated amount field anyway. The parser reads it and discards it without error.

**8. Failed movements are never stored, so they can never be disputed.**
If a deposit or withdrawal is rejected (insufficient funds, duplicate tx ID, account locked, overflow), the transaction record is never written to the tx store. A subsequent dispute referencing that ID will receive `UnknownTransaction` rather than some partially-applied state. Unknown clients always get an account record created even on a failed movement (the specification requires it), but verdict rows never create accounts.

**9. Amounts are stored as `Amount(i64)` minor units at a scale of 1e-4; there are no floats in the ledger path.**
`1.2345` is stored as `12345i64`. All arithmetic uses `checked_add`/`checked_sub`; an overflow produces a `Rejection::Overflow` rather than wrapping silently. The `Display` implementation always emits exactly four decimal places.

---

## Design

### Engine as library

The engine is a pure Rust library crate with no I/O dependencies. The public surface is small:

```
Engine::new()              → Engine
Engine::process(&mut self, Transaction) → Result<(), Rejection>
Engine::accounts(&self)    → impl Iterator<Item = (u16, &Account)>

io::read_transactions(impl Read) → impl Iterator<Item = Transaction>
io::write_accounts(iter, impl Write) → csv::Result<()>
io::run_sequential(impl Read) → Engine          // reference single-threaded path
runtime::run_sharded(impl Read) → Vec<Engine>   // sharded async runtime
testgen::generate(rows, seed, impl Write) → io::Result<()>
```

The CLI (`src/main.rs`) calls `run_sharded`, collects the resulting `Vec<Engine>`, flattens their `accounts()` iterators, and writes to stdout — a couple dozen lines with no business logic of its own.

### `Amount(i64)` and why no floats

IEEE 754 double precision cannot represent `0.1` exactly. Accumulating rounding errors across thousands of transactions would produce outputs that differ by sub-cent amounts depending on evaluation order. Storing amounts as integer minor units (1 unit = 0.0001 of the currency) eliminates this class of bug entirely. The scale is `10_000` (four decimal places as required by the specification). Parsing rejects any input with more than four fractional digits rather than rounding.

### Three-layer error taxonomy

| Layer | Type | Log level | Effect |
|-------|------|-----------|--------|
| 1 — fatal infrastructure | `anyhow::Error` | stderr (process exit) | Process exits non-zero |
| 2 — malformed row | `RowError` | `ERROR` | Row skipped; stream continues |
| 3 — ledger rejection | `Rejection` | `WARN` | Row ignored; no state change |

Layer 2 catches structural problems (bad CSV, unknown type, invalid amount string). Layer 3 catches business-rule violations (insufficient funds, wrong dispute state, client mismatch). These two layers never propagate upward; the stream is always resilient.

### Dispute state machine

Each stored transaction record carries one of three states:

```
         dispute
Undisputed ──────────────► Disputed
    ▲                          │
    │         resolve          │ chargeback
    └──────────────────────────┤
                               ▼
                          ChargedBack  (terminal — no further transitions)
```

- `dispute`: `Undisputed → Disputed`. Moves `amount` from `available` to `held`.
- `resolve`: `Disputed → Undisputed`. Moves `amount` from `held` back to `available`. The transaction can be disputed again.
- `chargeback`: `Disputed → ChargedBack`. Removes `amount` from `held` (zeroing it), locks the account. Terminal.

Any other combination (e.g. `chargeback` on `Undisputed`, `dispute` on `ChargedBack`) is a `Rejection` logged at `WARN`.

---

## Concurrency

### Architecture

```
 CSV file
    │
    ▼
 [blocking reader task]
    │  parse rows (RowErrors logged & skipped)
    │  route by client % 4
    ├──► bounded mpsc(1024) ──► [worker 0: Engine]
    ├──► bounded mpsc(1024) ──► [worker 1: Engine]
    ├──► bounded mpsc(1024) ──► [worker 2: Engine]
    └──► bounded mpsc(1024) ──► [worker 3: Engine]
                                        │
                          (senders drop on reader exit)
                                        │
                          [join all workers → Vec<Engine>]
                                        │
                                 [single writer]
                               CSV → stdout
```

The design in brief:

- **Per-client ordering by construction.** The only causal constraint in the data is per-client (a dispute must follow the movement it references; there are no transfers between accounts). Routing by `client % WORKERS` puts every transaction for a client into the same FIFO channel, consumed by a single worker — a dispute-before-deposit race is impossible, no locks needed. No state is shared: each `Engine` is moved into its worker and returned by value.
- **Bounded channels are the backpressure valve.** Capacity 1024; a lagging worker blocks the reader instead of buffering the file into memory. Plain modulo (not consistent hashing) suffices because the worker count is fixed per process.
- **Join, then write.** Output is the *final* ledger state, so the single writer runs only after all workers drain (reader EOF drops the senders, closing the channels).
- **Boundary:** per-client order is preserved within one input stream; across multiple streams it requires an upstream sequencing authority (e.g. Kafka keyed by `client_id`) — no consumer-side fix exists.
- **Replay idempotency:** duplicate IDs, re-disputes, and post-chargeback verdicts are all rejected without state change, so at-least-once redelivery converges. One scope note: duplicate-ID rejection is per-shard, so an ID reused across *different clients* on different shards would pass where the sequential reference rejects it — unreachable for conforming input (the spec guarantees globally unique IDs), and same-client replays always land on the same shard and are still rejected.

---

## Correctness

### Test inventory

The test suite has 43 tests: `cargo test` runs 42 (28 unit, 14 integration) and one `#[ignore]`d stress test runs at epoch gates.

| Module | Tests | Coverage |
|--------|-------|----------|
| `amount` | 4 | `parse_valid` (8 cases), `parse_invalid` (11 cases), `display_four_decimals`, `checked_ops` |
| `transaction` | 4 | valid movements and verdicts, superfluous amount tolerated, `client()` accessor, every `RowError` variant |
| `engine` | 15 | all ledger-rule `Rejection` variants (`Overflow` is exercised at the `Amount` layer via `checked_ops`), deposit/withdrawal arithmetic, all dispute state transitions including `reopened_dispute_can_charge_back`, `fraud_scenario_ends_negative_and_locked`, `withdrawal_dispute_uses_literal_spec_math`, `locked_account_rejects_movements_but_processes_verdicts`, `failed_withdrawal_creates_account`, `verdict_never_creates_account` |
| `io` | 3 | `reads_and_skips_malformed_rows`, `empty_input`, `end_to_end_sequential` |
| `testgen` | 2 | same seed produces identical bytes; generated output is processable and exercises disputes (open holds and locked accounts occur) |
| `acceptance` (integration) | 9 | 9 fixture scenarios (see below) |
| `cli` (integration) | 3 | binary contract: CSV to stdout on success, stdout stays empty on missing argument and on unreadable file |
| `equivalence` (integration) | 2 | sharded ≡ sequential property (see below) |
| `stress` (integration) | 1 | `#[ignore]`d big-file smoke (`five_million_rows_stream_through`), run at epoch gates via `cargo test --release --test stress -- --ignored` |

### Nine fixture scenarios

Each fixture is a hand-authored CSV paired with a hand-traced expected output:

| Fixture | What it exercises |
|---------|-------------------|
| `basic` | Multi-client deposit and failed withdrawal |
| `dispute_resolve` | Full dispute → resolve cycle |
| `chargeback_lock` | Dispute → chargeback → account locked |
| `withdrawal_dispute` | Disputing a withdrawal (literal-math premise) |
| `negative_balance` | Fraud pattern ending in negative total |
| `malformed` | Mix of valid and structurally broken rows |
| `noise` | Verdicts referencing unknown tx IDs |
| `whitespace` | CSV with extra spaces in fields |
| `empty` | Header-only and completely empty input |

### Sharded ≡ sequential equivalence

The `equivalence` integration tests verify that `run_sharded` and `run_sequential` produce identical sorted account rows for two inputs:

1. A deterministically generated 200,000-row CSV (seed 7) covering deposits, withdrawals, disputes, resolves, and chargebacks across 200 clients.
2. All nine fixture CSVs run through both runtimes back-to-back.

This property is the ordering guarantee: if sharding ever violated per-client FIFO, the balances would diverge from the sequential reference.

### Stress run numbers (Epoch 4 gate)

Measured on the reference machine — the 5-million-row figure via `cargo test --release --test stress -- --ignored`, the 10-million-row figures via a manual run of the release binary against a minted CSV at the epoch gate:

- **5 million rows**: 2.6 s (sequential reference)
- **10 million rows (281 MB input)**: 19.2 s sequential with default logging; **7.7 s with the shipped sharded runtime** (`RUST_LOG=error`), whose user CPU time (9.5 s) exceeding wall time confirms the reader and the four workers genuinely overlap. Peak RSS stays ~6 MB in both modes.

**What the 10 M run actually measures — be honest about it.**
The synthetic workload uses 200 clients with ~2% chargebacks. Because chargebacks lock accounts, all 200 accounts lock early in the stream; roughly 98% of subsequent deposit and withdrawal rows are rejected (`AccountLocked`) and — by design — never stored. As a result, the transaction store stops growing after the first few hundred stored movements. The 5.2 MB peak RSS is real, but it reflects **streaming throughput and rejection-path correctness** (a 281 MB file is never buffered), NOT store growth. It is not representative of a realistic unlocked workload.

A **lock-free realistic workload** was measured separately: 4 million stored movements peaked at roughly **417 MB RSS**, giving an observed cost of approximately **104 bytes per stored transaction all-in** (HashMap overhead included).

Wall time for the 10 M run was partly dominated by millions of `WARN` lines emitted to stderr. For rejection-heavy feeds, `RUST_LOG=error cargo run --release -- big.csv > /dev/null` is the sensible setting and produces materially faster wall times.

---

## Scaling Ceiling

### Transaction store memory

Every successfully committed movement (deposit or withdrawal) stores a `TxRecord`: `client: u16` (2 bytes) + `amount: i64` (8 bytes) + `DisputeState: u8` (1 byte) + HashMap overhead. Measured at scale (4 M stored movements, lock-free workload) the all-in cost is approximately **104 bytes per stored transaction**:

| Movements | Approximate RSS |
|-----------|----------------|
| 1 million | ~104 MB |
| 10 million | ~1 GB |
| 100 million | ~10 GB |

The transaction store is irreducible state: a dispute arriving at row N could reference any earlier row, so records cannot be evicted without closing the dispute window.

### Escape hatch: disk-backed KV

For workloads exceeding available RAM, the `txs: HashMap<u32, TxRecord>` can be replaced with a disk-backed key-value store (e.g. RocksDB via the `rocksdb` crate). The rest of the engine is unchanged because the only access pattern is point get and point insert — both map cleanly onto KV semantics. The accounts map is much smaller (one entry per client) and stays in memory.

### What was deliberately not implemented: two-pass pre-scan

A two-pass approach — first scan to find all disputed IDs, second pass to process — was considered and rejected. It requires the entire input to be seekable (breaks piped or streamed input), doubles I/O, and still does not bound memory if the number of disputed transactions is large. The streaming single-pass design is strictly better for the target use case.

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

The suite has 43 tests: unit tests for the money type and every ledger rule, nine fixture scenarios with hand-traced expected outputs (disputes on both movement types, chargeback locking, negative balances, malformed and noisy input, whitespace, empty files), and equivalence tests proving the sharded runtime produces identical results to the sequential reference — on all fixtures and on 200,000 generated rows. That equivalence is the per-client ordering guarantee made executable.

---

## Sequential vs. concurrent

Same 10-million-row (281 MB) input, same 201-account output, measured on the reference machine:

| Runtime | Wall time | Peak RSS |
|---|---|---|
| Sequential (library reference, default logging) | ~19 s | ~5 MB |
| Concurrent, 4 workers (shipped binary, `RUST_LOG=error`) | **7.7 s** | ~6 MB |

Part of the gap is quieter logging (the sequential run paid for millions of stderr `WARN` lines); the rest is real overlap.

User CPU time (9.5 s) exceeding wall time in the concurrent run confirms the reader and workers genuinely overlap. Memory stays flat in both modes because the input is streamed, never buffered. The transaction store is the one irreducible piece of state (a dispute at row N can reference any earlier row — measured cost ~104 bytes per stored movement); for workloads beyond available RAM it could be swapped for a disk-backed KV store without touching the rest of the engine.

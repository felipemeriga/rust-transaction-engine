use crate::{engine::Engine, io::read_transactions};
use std::io::Read;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

pub const WORKERS: usize = 4;
const CHANNEL_CAPACITY: usize = 1024;

/// Sharded concurrent runtime: a single blocking reader routes transactions
/// by `client % WORKERS` into bounded mpsc channels. Each worker task
/// exclusively owns an Engine, processes its shard's transactions in file
/// order, and returns the engine by value. No shared state anywhere.
///
/// Per-client ordering is preserved because a given client always maps to
/// the same shard, whose worker drains its channel sequentially.
pub async fn run_sharded(input: impl Read + Send + 'static) -> Vec<Engine> {
    let mut senders = Vec::with_capacity(WORKERS);
    let mut workers = JoinSet::new();

    for _ in 0..WORKERS {
        let (tx, mut rx) = mpsc::channel(CHANNEL_CAPACITY);
        senders.push(tx);
        workers.spawn(async move {
            let mut engine = Engine::new();
            while let Some(t) = rx.recv().await {
                if let Err(rejection) = engine.process(t) {
                    log::warn!("{rejection}");
                }
            }
            engine
        });
    }

    // Parse errors are handled inside read_transactions (log-and-skip), so
    // workers only ever see valid Transactions.
    let reader = tokio::task::spawn_blocking(move || {
        for t in read_transactions(input) {
            let shard = t.client() as usize % WORKERS;
            if senders[shard].blocking_send(t).is_err() {
                break; // worker gone — stop reading
            }
        }
        // Senders dropped here: EOF for every worker.
    });

    reader.await.expect("reader task panicked");

    let mut engines = Vec::with_capacity(WORKERS);
    while let Some(engine) = workers.join_next().await {
        engines.push(engine.expect("worker panicked"));
    }
    engines
}

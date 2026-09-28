use crate::{engine::Engine, io::read_transactions};
use std::io::Read;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

const WORKERS: usize = 4;
const CHANNEL_CAPACITY: usize = 1024;

/// Sharded concurrent runtime: routes transactions by client % WORKERS into
/// bounded mpsc channels. Each worker task exclusively owns an Engine,
/// processes its shard's transactions, and returns the engine by value.
/// Output engines are merged by client into a single final Engine.
pub async fn run_sharded(
    input: impl Read + Send + 'static,
) -> Result<Engine, Box<dyn std::error::Error>> {
    // Create channels for each worker shard
    let mut senders = Vec::with_capacity(WORKERS);
    let mut receivers = Vec::with_capacity(WORKERS);

    for _ in 0..WORKERS {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        senders.push(tx);
        receivers.push(rx);
    }

    // Spawn reader task that routes transactions by client % WORKERS
    let reader_handle = tokio::task::spawn_blocking(move || {
        for t in read_transactions(input) {
            let shard_idx = (t.client() as usize) % WORKERS;
            // If send fails (channel closed), reader is done
            if senders[shard_idx].blocking_send(t).is_err() {
                break;
            }
        }
        // Senders dropped here, all channels closed
    });

    // Spawn worker tasks: each owns an engine, processes its shard's transactions
    let mut join_set = JoinSet::new();

    for rx in receivers {
        join_set.spawn(async move {
            let mut engine = Engine::new();
            let mut rx = rx;

            while let Some(t) = rx.recv().await {
                if let Err(rejection) = engine.process(t) {
                    log::warn!("{rejection}");
                }
            }

            engine
        });
    }

    // Wait for reader to complete
    reader_handle.await?;

    // Collect completed engines from workers
    let mut engines = Vec::with_capacity(WORKERS);
    while let Some(engine) = join_set.join_next().await {
        engines.push(engine?);
    }

    // Merge engines by client: combine all accounts from all shards
    let mut final_engine = Engine::new();
    for engine in engines {
        for (client, account) in engine.accounts() {
            final_engine.merge_account(client, account);
        }
    }

    Ok(final_engine)
}

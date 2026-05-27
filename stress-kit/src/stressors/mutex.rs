//! Mutex-contention stressor.
//!
//! All workers fight over a single `Mutex<u64>`, performing a small
//! read-modify-write under the lock. The point is to exercise the OS
//! mutex (futex on Linux, SRWLock on Windows) — not the work itself.
//! Reports Mops/s of lock-unlock pairs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const OPS_PER_BURST: u64 = 50_000;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let shared = Arc::new(Mutex::new(0u64));
    let ops_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .enumerate()
        .map(|(id, _)| {
            let cancel = cancel.clone();
            let shared = shared.clone();
            let ops_counter = ops_counter.clone();
            thread::Builder::new()
                .name("stress-kit-mutex".into())
                .spawn(move || mutex_worker(id as u64, cancel, shared, ops_counter))
                .expect("stress-kit: failed to spawn mutex worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_ops: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_ops = ops_counter.load(Ordering::Relaxed);
            let delta = now_ops.saturating_sub(last_ops);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mops = (delta as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mops,
                last_error: None,
                fatal: false,
            });

            last_ops = now_ops;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn mutex_worker(
    id: u64,
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<u64>>,
    ops_counter: Arc<AtomicU64>,
) {
    let nudge = (id & 0xFF).wrapping_add(1);
    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..OPS_PER_BURST {
            if let Ok(mut g) = shared.lock() {
                let v = *g;
                *g = v.wrapping_add(nudge).rotate_left(1);
                std::hint::black_box(&*g);
            } else {
                // Poisoned — the run is still useful, just bail this burst.
                return;
            }
        }
        ops_counter.fetch_add(OPS_PER_BURST, Ordering::Relaxed);
    }
}

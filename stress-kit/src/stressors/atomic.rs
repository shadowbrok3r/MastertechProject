//! Atomic contention stressor.
//!
//! All workers hammer the *same* `AtomicU64` with a rotating mix of
//! `fetch_add`, `fetch_xor`, `fetch_or`, and `compare_exchange`. This is the
//! "all cores fighting over one cache line" worst case for the coherency
//! protocol. Reports Mops/s of atomic operations issued.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const OPS_PER_BURST: u64 = 200_000;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let target = Arc::new(AtomicU64::new(0));
    let ops_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .enumerate()
        .map(|(id, _)| {
            let cancel = cancel.clone();
            let target = target.clone();
            let ops_counter = ops_counter.clone();
            thread::Builder::new()
                .name("stress-kit-atomic".into())
                .spawn(move || atomic_worker(id as u64, cancel, target, ops_counter))
                .expect("stress-kit: failed to spawn atomic worker")
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

    log::debug!(
        "stress-kit/atomic: final target value = {}",
        target.load(Ordering::Relaxed)
    );
}

fn atomic_worker(
    id: u64,
    cancel: Arc<AtomicBool>,
    target: Arc<AtomicU64>,
    ops_counter: Arc<AtomicU64>,
) {
    let lane = (id & 0xFF).wrapping_add(1);
    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..OPS_PER_BURST / 4 {
            target.fetch_add(lane, Ordering::Relaxed);
            target.fetch_xor(0xAA55_AA55_AA55_AA55, Ordering::Relaxed);
            target.fetch_or(0x0001_0000, Ordering::Relaxed);

            // CAS dance — keep the loop bounded so a hot core can't starve.
            let mut spins = 0u32;
            loop {
                let cur = target.load(Ordering::Relaxed);
                let next = cur.wrapping_sub(lane).rotate_left(1);
                if target
                    .compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    break;
                }
                spins += 1;
                if spins > 32 {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        ops_counter.fetch_add(OPS_PER_BURST, Ordering::Relaxed);
    }
}

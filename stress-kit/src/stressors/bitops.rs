//! Tight bitops loop: popcount, leading/trailing zeros, rotate. Pure Rust, no syscalls.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const OPS_PER_BURST: u64 = 1_000_000;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let burst_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = burst_counter.clone();
            thread::Builder::new()
                .name("stress-kit-bitops".into())
                .spawn(move || bitops_worker(cancel, counter))
                .expect("stress-kit: failed to spawn bitops worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_count = burst_counter.load(Ordering::Relaxed);
            let delta_bursts = now_count.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            // 4 ops per inner iter (popcount, clz, ctz, rotate) × burst size.
            let mops = (delta_bursts as f64 * OPS_PER_BURST as f64 * 4.0) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mops,
                last_error: None,
                fatal: false,
            });

            last_count = now_count;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn bitops_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut x: u64 = 0x0123_4567_89AB_CDEF;

    loop {
        for _ in 0..OPS_PER_BURST {
            let pc = x.count_ones() as u64;
            let lz = x.leading_zeros() as u64;
            let tz = x.trailing_zeros() as u64;
            x = x.rotate_left(7) ^ pc ^ (lz << 3) ^ (tz << 11);
        }
        std::hint::black_box(x);
        counter.fetch_add(1, Ordering::Relaxed);

        if cancel.load(Ordering::Relaxed) {
            break;
        }
    }
}

//! Floating-point / FMA chain stressor.
//!
//! Each worker keeps 8 independent FMA chains running in parallel — this
//! exposes the FMA unit's pipeline depth and lets the optimizer fuse the
//! `mul_add` calls. Reports Mflop/s (counting 2 flops per FMA).
//!
//! `f64::mul_add` is `vfmadd*` on x86_64 with FMA, and the LLVM IR lowers to
//! a real fused op on every modern CPU we ship for. On exotic targets it
//! falls back to a separate `mul`+`add`, which still produces a useful (if
//! lower) signal.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const CHAIN_DEPTH: usize = 8;
const ITERS_PER_BURST: u64 = 200_000;
const FLOPS_PER_BURST: u64 = ITERS_PER_BURST * CHAIN_DEPTH as u64 * 2; // 2 flops per FMA.
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
                .name("stress-kit-fp".into())
                .spawn(move || fp_worker(cancel, counter))
                .expect("stress-kit: failed to spawn fp worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = burst_counter.load(Ordering::Relaxed);
            let delta = now.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mflops = (delta as f64 * FLOPS_PER_BURST as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mflops,
                last_error: None,
                fatal: false,
            });

            last_count = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn fp_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut acc = [
        1.000_001_f64,
        1.000_002,
        1.000_003,
        1.000_004,
        1.000_005,
        1.000_006,
        1.000_007,
        1.000_008,
    ];
    let mul = [
        1.000_000_001_f64,
        1.000_000_002,
        1.000_000_003,
        1.000_000_004,
        1.000_000_005,
        1.000_000_006,
        1.000_000_007,
        1.000_000_008,
    ];
    let add = [
        0.000_000_001_f64,
        0.000_000_002,
        0.000_000_003,
        0.000_000_004,
        0.000_000_005,
        0.000_000_006,
        0.000_000_007,
        0.000_000_008,
    ];

    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..ITERS_PER_BURST {
            // 8 independent FMA chains — let the optimizer issue them in parallel.
            for i in 0..CHAIN_DEPTH {
                acc[i] = acc[i].mul_add(mul[i], add[i]);
            }
        }

        // Renormalize so we don't drift into Inf/NaN over a long run.
        for i in 0..CHAIN_DEPTH {
            if !acc[i].is_finite() || acc[i].abs() > 1e30 {
                acc[i] = 1.0 + (i as f64) * 1e-6;
            }
        }
        std::hint::black_box(&acc);
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

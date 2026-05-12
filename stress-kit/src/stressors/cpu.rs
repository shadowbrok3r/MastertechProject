//! One worker thread per logical core: float bursts (`sin`/`cos`/`sqrt`), shared op counter for Mop/s.
//! Cancel is checked once per [`OPS_PER_BURST`] inner iterations.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

pub(crate) const OPS_PER_BURST: u64 = 500_000;

const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = counter.clone();
            thread::Builder::new()
                .name("stress-kit-cpu".into())
                .spawn(move || cpu_worker(cancel, counter))
                .expect("stress-kit: failed to spawn cpu worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_count = counter.load(Ordering::Relaxed);
            let delta_bursts = now_count.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mops_per_sec = (delta_bursts as f64 * OPS_PER_BURST as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mops_per_sec,
                last_error: None,
            });

            last_count = now_count;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn cpu_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut x: f64 = std::f64::consts::E;

    loop {
        for _ in 0..OPS_PER_BURST {
            x = (x * 1.000_001_f64).sqrt();
            x = x.sin().abs() + 1.001;
            x = x.cos().abs() + 1.001;
        }

        if !x.is_finite() || x < 0.5 {
            x = std::f64::consts::E;
        }

        std::hint::black_box(x);

        counter.fetch_add(1, Ordering::Relaxed);

        if cancel.load(Ordering::Relaxed) {
            break;
        }
    }
}

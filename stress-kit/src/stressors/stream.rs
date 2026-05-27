//! STREAM-style memory-bandwidth stressor (McCalpin).
//!
//! Each worker owns three `N`-element `f64` arrays (a, b, c) and rotates
//! through `copy`, `scale`, `add`, `triad`. The aggregate "bytes moved"
//! counter is reported in GB/s (decimal, like McCalpin's original).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const N: usize = 2 * 1024 * 1024;
const F64_BYTES: usize = std::mem::size_of::<f64>();
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let bytes_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = bytes_counter.clone();
            thread::Builder::new()
                .name("stress-kit-stream".into())
                .spawn(move || stream_worker(cancel, counter))
                .expect("stress-kit: failed to spawn stream worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_bytes: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_bytes = bytes_counter.load(Ordering::Relaxed);
            let delta = now_bytes.saturating_sub(last_bytes);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let gb_per_sec = (delta as f64) / 1_000_000_000.0 / delta_secs;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: gb_per_sec,
                last_error: None,
                fatal: false,
            });

            last_bytes = now_bytes;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn stream_worker(cancel: Arc<AtomicBool>, bytes_counter: Arc<AtomicU64>) {
    let mut a = vec![1.0_f64; N];
    let mut b = vec![2.0_f64; N];
    let mut c = vec![0.0_f64; N];
    let scalar = 3.0_f64;

    // copy:  c = a               -> 2N bytes (1 read + 1 write)
    // scale: b = scalar * c      -> 2N bytes
    // add:   c = a + b           -> 3N bytes
    // triad: a = b + scalar * c  -> 3N bytes
    let bytes_copy = (2 * N * F64_BYTES) as u64;
    let bytes_scale = (2 * N * F64_BYTES) as u64;
    let bytes_add = (3 * N * F64_BYTES) as u64;
    let bytes_triad = (3 * N * F64_BYTES) as u64;

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        for j in 0..N {
            c[j] = a[j];
        }
        bytes_counter.fetch_add(bytes_copy, Ordering::Relaxed);
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        for j in 0..N {
            b[j] = scalar * c[j];
        }
        bytes_counter.fetch_add(bytes_scale, Ordering::Relaxed);
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        for j in 0..N {
            c[j] = a[j] + b[j];
        }
        bytes_counter.fetch_add(bytes_add, Ordering::Relaxed);
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        for j in 0..N {
            a[j] = b[j] + scalar * c[j];
        }
        bytes_counter.fetch_add(bytes_triad, Ordering::Relaxed);

        // Keep the optimizer honest: every loop, perturb the head element so
        // it can't hoist the writes.
        a[0] = a[0].mul_add(1.000_000_001, 0.000_000_1);
        std::hint::black_box((&a, &b, &c));
    }
}

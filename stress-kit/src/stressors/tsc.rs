//! Timestamp-counter read-rate stressor.
//!
//! Hammer `rdtsc` (or the best portable equivalent) and count reads/sec.
//! Reports Mread/s. On x86_64 this exercises the TSC hardware and the
//! kernel's no-op `rdtsc` path; on other architectures we fall back to
//! `std::time::Instant::now`, which still measures the OS time-stamp path.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const READS_PER_BURST: u64 = 200_000;
const TICK: Duration = Duration::from_millis(500);

#[cfg(target_arch = "x86_64")]
#[inline]
fn read_tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn read_tsc() -> u64 {
    use std::time::Instant;
    Instant::now().elapsed().as_nanos() as u64
}

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let reads_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = reads_counter.clone();
            thread::Builder::new()
                .name("stress-kit-tsc".into())
                .spawn(move || tsc_worker(cancel, counter))
                .expect("stress-kit: failed to spawn tsc worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = reads_counter.load(Ordering::Relaxed);
            let delta = now.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mread = (delta as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mread,
                last_error: None,
                fatal: false,
                errors: 0,
            });

            last_count = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn tsc_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut acc: u64 = 0;
    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..READS_PER_BURST {
            // Black-box the result so the optimizer doesn't elide the read.
            acc = acc.wrapping_add(std::hint::black_box(read_tsc()));
        }
        std::hint::black_box(acc);
        counter.fetch_add(READS_PER_BURST, Ordering::Relaxed);
    }
}

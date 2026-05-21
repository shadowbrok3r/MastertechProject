//! Branch-predictor stressor.
//!
//! Iterate over a `BUF_LEN` array of pseudo-random `u32`s, taking a branch
//! that depends on the data so the predictor can't memoize the pattern.
//! Reports Mbranch/s — millions of *executed* conditional branches per second
//! (taken or not). The xorshift makes the run deterministic for a given seed.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const BUF_LEN: usize = 64 * 1024; // 256 KiB — bigger than L1, lives in L2/L3.
const BRANCHES_PER_BURST: u64 = BUF_LEN as u64;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let burst_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .enumerate()
        .map(|(id, _)| {
            let cancel = cancel.clone();
            let counter = burst_counter.clone();
            thread::Builder::new()
                .name("stress-kit-branch".into())
                .spawn(move || branch_worker(id as u64, cancel, counter))
                .expect("stress-kit: failed to spawn branch worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_count = burst_counter.load(Ordering::Relaxed);
            let delta = now_count.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mbranch = (delta as f64 * BRANCHES_PER_BURST as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mbranch,
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

fn branch_worker(seed: u64, cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    // Pre-fill a pseudo-random pattern. Splitmix gives a good per-thread seed.
    let mut state = splitmix64(0xDEAD_BEEF_CAFE_BABE ^ seed);
    let mut buf: Vec<u32> = Vec::with_capacity(BUF_LEN);
    for _ in 0..BUF_LEN {
        state = splitmix64(state);
        buf.push((state >> 32) as u32);
    }

    let mut taken_acc: u64 = 0;
    let mut not_taken_acc: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        let mut x: u32 = 0x12345678;
        for &v in buf.iter() {
            // Data-dependent branch: optimizer can't fold it because the
            // counters escape via black_box at the end of each burst.
            if (v ^ x).count_ones() & 1 == 0 {
                taken_acc = taken_acc.wrapping_add(v as u64);
                x = x.rotate_left(7) ^ v;
            } else {
                not_taken_acc = not_taken_acc.wrapping_add(v as u64);
                x = x.rotate_right(5).wrapping_add(v);
            }
        }
        std::hint::black_box((taken_acc, not_taken_acc, x));
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

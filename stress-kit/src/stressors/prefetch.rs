//! Hardware-prefetch stressor.
//!
//! Two phases per burst on the *same* buffer:
//!   1. Sequential stride read — the prefetcher can lock onto the pattern.
//!   2. Pseudo-random stride read — defeats the prefetcher.
//!
//! Both phases sum cache-line-aligned bytes. The throughput we report is the
//! aggregate Mref/s; the *difference* between the two phases is what a human
//! eyeballing perf counters cares about, but for stress purposes we want the
//! buffer hammered hard end-to-end.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const WORKING_SET_BYTES: usize = 32 * 1024 * 1024;
const CACHE_LINE: usize = 64;
const LINES: usize = WORKING_SET_BYTES / CACHE_LINE;
const REFS_PER_BURST: u64 = (LINES as u64) * 2; // two phases per burst.
const TICK: Duration = Duration::from_millis(500);

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
#[inline]
unsafe fn prefetch_t0(p: *const u8) {
    use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
    unsafe { _mm_prefetch::<_MM_HINT_T0>(p as *const i8) };
}

#[cfg(not(all(target_arch = "x86_64", target_feature = "sse2")))]
#[inline]
unsafe fn prefetch_t0(_p: *const u8) {}

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
                .name("stress-kit-prefetch".into())
                .spawn(move || prefetch_worker(cancel, counter))
                .expect("stress-kit: failed to spawn prefetch worker")
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
            let mref = (delta as f64 * REFS_PER_BURST as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mref,
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

fn prefetch_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut buf = vec![0u8; WORKING_SET_BYTES];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }

    let base = buf.as_ptr();
    let len = buf.len();
    // 4099 is prime → walks every line over LINES iterations.
    let stride_lines: usize = 4099;

    while !cancel.load(Ordering::Relaxed) {
        // Phase 1: sequential. Prefetcher should be happy here.
        let mut acc: u8 = 0;
        let mut off = 0usize;
        while off < len {
            unsafe {
                let p = base.add(off);
                // Hint the next line ahead so even tiny L1s benefit.
                if off + CACHE_LINE * 8 < len {
                    prefetch_t0(p.add(CACHE_LINE * 8));
                }
                acc ^= *p;
            }
            off += CACHE_LINE;
        }
        std::hint::black_box(acc);
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        // Phase 2: striped pseudo-random. Prefetcher can't lock on.
        let mut acc2: u8 = 0;
        let mut line: usize = 0;
        for _ in 0..LINES {
            let off = line * CACHE_LINE;
            unsafe {
                acc2 ^= *base.add(off);
            }
            line = (line + stride_lines) % LINES;
        }
        std::hint::black_box(acc2);
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

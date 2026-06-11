//! Cache-line thrash. On x86_64 with SSE2 (default for the `*-pc-windows-msvc`
//! target) we use `_mm_prefetch` + `_mm_clflush` + `_mm_mfence` on a working set
//! that spills L1/L2. On other targets we fall back to a striding read loop.
//!
//! Throughput is reported in millions-of-references-per-second (`Mref/s`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const WORKING_SET_BYTES: usize = 16 * 1024 * 1024;
const CACHE_LINE: usize = 64;
const LINES: usize = WORKING_SET_BYTES / CACHE_LINE;
const REFS_PER_BURST: u64 = LINES as u64;
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
                .name("stress-kit-cache".into())
                .spawn(move || cache_worker(cancel, counter))
                .expect("stress-kit: failed to spawn cache worker")
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
            let mref = (delta as f64 * REFS_PER_BURST as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mref,
                last_error: None,
                fatal: false,
                errors: 0,
            });

            last_count = now_count;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn cache_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut buf = vec![0u8; WORKING_SET_BYTES];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        thrash(&mut buf);
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
fn thrash(buf: &mut [u8]) {
    use std::arch::x86_64::{_mm_clflush, _mm_mfence, _mm_prefetch, _MM_HINT_T0};

    let len = buf.len();
    let base = buf.as_mut_ptr();
    let mut acc: u8 = 0;
    let mut offset: usize = 0;
    while offset < len {
        unsafe {
            let p = base.add(offset);
            _mm_prefetch::<_MM_HINT_T0>(p as *const i8);
            acc ^= *p;
            _mm_clflush(p);
        }
        offset += CACHE_LINE;
    }
    unsafe {
        _mm_mfence();
    }
    std::hint::black_box(acc);
}

#[cfg(not(all(target_arch = "x86_64", target_feature = "sse2")))]
fn thrash(buf: &mut [u8]) {
    let len = buf.len();
    let mut acc: u8 = 0;
    let mut offset: usize = 0;
    while offset < len {
        acc ^= buf[offset];
        offset += CACHE_LINE;
    }
    std::hint::black_box(acc);
}

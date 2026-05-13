//! Paired-buffer memcpy stress. Worker alternates `ptr::copy_nonoverlapping`
//! and `copy_from_slice` on a `BUF_BYTES`-sized pair, counts bytes moved.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const BUF_BYTES: usize = 4 * 1024 * 1024;
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
                .name("stress-kit-memcpy".into())
                .spawn(move || memcpy_worker(cancel, counter))
                .expect("stress-kit: failed to spawn memcpy worker")
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
            });

            last_bytes = now_bytes;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn memcpy_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut src = vec![0u8; BUF_BYTES];
    let mut dst = vec![0u8; BUF_BYTES];

    for (i, b) in src.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }

    let mut toggle = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        if toggle {
            dst.copy_from_slice(&src);
        } else {
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), BUF_BYTES);
            }
        }
        toggle = !toggle;

        std::hint::black_box(dst.as_ptr());
        counter.fetch_add(BUF_BYTES as u64, Ordering::Relaxed);

        // Mutate one byte so the optimizer cannot lift the copy.
        src[0] = src[0].wrapping_add(1);
    }
}

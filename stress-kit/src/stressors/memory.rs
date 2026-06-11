//! Alloc/touch `CHUNK_MB` blocks up to `memory_cap_mb` per thread, then replace one chunk per loop.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const CHUNK_MB: u64 = 16;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let cap_per_thread_mb = (memory_cap_mb / thread_count.max(1) as u64).max(CHUNK_MB);
    let bytes_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let bytes_counter = bytes_counter.clone();
            thread::Builder::new()
                .name("stress-kit-mem".into())
                .spawn(move || memory_worker(cancel, bytes_counter, cap_per_thread_mb))
                .expect("stress-kit: failed to spawn memory worker")
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
            let mib_per_sec = (delta as f64) / (1024.0 * 1024.0) / delta_secs;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error: None,
                fatal: false,
                errors: 0,
            });

            last_bytes = now_bytes;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn memory_worker(cancel: Arc<AtomicBool>, bytes_counter: Arc<AtomicU64>, cap_mb: u64) {
    let chunk_bytes = CHUNK_MB as usize * 1024 * 1024;
    let max_chunks = (cap_mb / CHUNK_MB).max(1) as usize;

    // Pre-allocate the capped set of chunks up front; this is the "hold" phase.
    // The churn below reallocates one chunk at a time to stress the allocator.
    let mut chunks: Vec<Vec<u8>> = Vec::with_capacity(max_chunks);

    for _ in 0..max_chunks {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut chunk = vec![0u8; chunk_bytes];
        write_pattern(&mut chunk);
        bytes_counter.fetch_add(chunk_bytes as u64, Ordering::Relaxed);
        chunks.push(chunk);
    }

    let mut idx = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        chunks[idx] = {
            let mut chunk = vec![0u8; chunk_bytes];
            write_pattern(&mut chunk);
            bytes_counter.fetch_add(chunk_bytes as u64, Ordering::Relaxed);
            chunk
        };

        idx = (idx + 1) % max_chunks;
    }

    drop(chunks);
}

#[inline]
fn write_pattern(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8 ^ 0xA5;
    }
    std::hint::black_box(&*buf);
}

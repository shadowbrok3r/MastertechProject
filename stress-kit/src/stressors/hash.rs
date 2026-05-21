//! Hash-mixing stressor.
//!
//! Each worker runs FNV-1a 64-bit over a 1 MiB buffer per pass, then mutates
//! the head bytes so the optimizer can't lift the hash. Reports MiB/s of
//! data hashed. This is a "mostly L2/L3 + ALU" workload — good for catching
//! marginal cache silicon that the pure-CPU `Cpu` stressor sails through.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const BUF_BYTES: usize = 1024 * 1024;
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
                .name("stress-kit-hash".into())
                .spawn(move || hash_worker(cancel, counter))
                .expect("stress-kit: failed to spawn hash worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_bytes: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = bytes_counter.load(Ordering::Relaxed);
            let delta = now.saturating_sub(last_bytes);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mib_per_sec = (delta as f64) / (1024.0 * 1024.0) / delta_secs;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error: None,
            });

            last_bytes = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn hash_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut buf = vec![0u8; BUF_BYTES];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }

    let mut tag: u8 = 0;
    while !cancel.load(Ordering::Relaxed) {
        let h = fnv1a64(&buf);
        std::hint::black_box(h);
        counter.fetch_add(BUF_BYTES as u64, Ordering::Relaxed);

        // Mutate the leading cache line so the next pass isn't a no-op.
        tag = tag.wrapping_add(1);
        for b in buf[0..64].iter_mut() {
            *b ^= tag;
        }
    }
}

#[inline]
fn fnv1a64(buf: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in buf {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

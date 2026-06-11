//! Pattern write/verify memory tester. Reports MiB/s; mismatches accumulate
//! in `Metrics::errors` with detail in `last_error`.
//!
//! Each pass cycles the classic memtest patterns: moving inversions over
//! solid and checkerboard patterns, a rotating walking-ones window,
//! address-in-address, and a seeded xorshift random sequence. Verification
//! sums mismatches branchlessly per chunk, then re-scans the chunk for the
//! offending word only when the count moved.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const TICK: Duration = Duration::from_millis(500);
const MIN_WORKER_MB: u64 = 16;
/// Words verified between cancel checks and detail re-scans (32 KiB).
const CHUNK_WORDS: usize = 4096;
/// Walking-ones shifts exercised per pass cycle.
const WALK_SHIFTS_PER_PASS: u64 = 8;

pub(crate) fn run(
    thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let cap_per_thread_mb = (memory_cap_mb / thread_count.max(1) as u64).max(MIN_WORKER_MB);
    let bytes_counter = Arc::new(AtomicU64::new(0));
    let error_counter = Arc::new(AtomicU64::new(0));
    let error_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let handles: Vec<_> = (0..thread_count)
        .map(|worker_id| {
            let cancel = cancel.clone();
            let bytes_counter = bytes_counter.clone();
            let error_counter = error_counter.clone();
            let error_slot = error_slot.clone();
            thread::Builder::new()
                .name("stress-kit-memtest".into())
                .spawn(move || {
                    memtest_worker(
                        worker_id,
                        cancel,
                        bytes_counter,
                        error_counter,
                        error_slot,
                        cap_per_thread_mb,
                    )
                })
                .expect("stress-kit: failed to spawn memtest worker")
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
            let errors = error_counter.load(Ordering::Relaxed);
            let last_error = error_slot.lock().ok().and_then(|g| g.clone());

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error,
                fatal: false,
                errors,
            });

            last_bytes = now_bytes;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

struct ErrorSink {
    worker_id: usize,
    counter: Arc<AtomicU64>,
    slot: Arc<Mutex<Option<String>>>,
}

impl ErrorSink {
    fn record(&self, pass: &str, word_index: usize, expected: u64, got: u64, extra: u64) {
        self.counter.fetch_add(1 + extra, Ordering::Relaxed);
        let msg = format!(
            "memtest[{}] {}: offset 0x{:X} expected 0x{:016X} got 0x{:016X}{}",
            self.worker_id,
            pass,
            word_index * 8,
            expected,
            got,
            if extra > 0 {
                format!(" (+{extra} more in chunk)")
            } else {
                String::new()
            }
        );
        log::error!("[stress-kit/memtest] {msg}");
        if let Ok(mut g) = self.slot.lock() {
            *g = Some(msg);
        }
    }
}

fn memtest_worker(
    worker_id: usize,
    cancel: Arc<AtomicBool>,
    bytes_counter: Arc<AtomicU64>,
    error_counter: Arc<AtomicU64>,
    error_slot: Arc<Mutex<Option<String>>>,
    cap_mb: u64,
) {
    let sink = ErrorSink {
        worker_id,
        counter: error_counter,
        slot: error_slot,
    };

    // Halve on allocation failure rather than aborting the test.
    let mut words = (cap_mb as usize) * 1024 * 1024 / 8;
    let mut buf: Vec<u64> = loop {
        let mut v: Vec<u64> = Vec::new();
        if v.try_reserve_exact(words).is_ok() {
            v.resize(words, 0);
            break v;
        }
        if words <= (MIN_WORKER_MB as usize) * 1024 * 1024 / 8 {
            log::warn!("[stress-kit/memtest] worker {worker_id}: allocation failed, exiting");
            return;
        }
        words /= 2;
    };

    let mut pass: u64 = 0;
    while !cancel.load(Ordering::Relaxed) {
        let seed = 0x9E37_79B9_7F4A_7C15u64 ^ (worker_id as u64) << 32 ^ pass;

        moving_inversion(&mut buf, 0, &sink, &bytes_counter, &cancel, "solid");
        moving_inversion(
            &mut buf,
            0x5555_5555_5555_5555,
            &sink,
            &bytes_counter,
            &cancel,
            "checkerboard",
        );

        let shift_base = (pass * WALK_SHIFTS_PER_PASS) % 64;
        for s in 0..WALK_SHIFTS_PER_PASS {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let pattern = 1u64 << ((shift_base + s) % 64);
            moving_inversion(&mut buf, pattern, &sink, &bytes_counter, &cancel, "walking-1");
        }

        address_in_address(&mut buf, seed, &sink, &bytes_counter, &cancel);
        random_pattern(&mut buf, seed, &sink, &bytes_counter, &cancel);

        pass = pass.wrapping_add(1);
    }

    drop(buf);
}

/// Write `pattern` ascending, verify+write `!pattern` ascending, verify+write
/// `pattern` descending.
fn moving_inversion(
    buf: &mut [u64],
    pattern: u64,
    sink: &ErrorSink,
    bytes: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
    pass: &str,
) {
    let inverse = !pattern;

    for chunk in buf.chunks_mut(CHUNK_WORDS) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        for w in chunk.iter_mut() {
            *w = pattern;
        }
        bytes.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
    }
    std::hint::black_box(&*buf);

    sweep(buf, pattern, inverse, false, sink, bytes, cancel, pass);
    std::hint::black_box(&*buf);
    sweep(buf, inverse, pattern, true, sink, bytes, cancel, pass);
    std::hint::black_box(&*buf);
}

/// Verify each word equals `expected`, then overwrite with `write`. Chunked:
/// a branchless mismatch count first, a scalar re-scan only when it moved.
#[allow(clippy::too_many_arguments)]
fn sweep(
    buf: &mut [u64],
    expected: u64,
    write: u64,
    descending: bool,
    sink: &ErrorSink,
    bytes: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
    pass: &str,
) {
    let total = buf.len();
    let chunk_count = total.div_ceil(CHUNK_WORDS);

    for c in 0..chunk_count {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let c = if descending { chunk_count - 1 - c } else { c };
        let start = c * CHUNK_WORDS;
        let end = (start + CHUNK_WORDS).min(total);
        let chunk = &mut buf[start..end];

        let mut mismatches: u64 = 0;
        for w in chunk.iter() {
            mismatches += (*w != expected) as u64;
        }
        if mismatches > 0 {
            if let Some((i, got)) = chunk
                .iter()
                .enumerate()
                .find(|(_, w)| **w != expected)
                .map(|(i, w)| (i, *w))
            {
                sink.record(pass, start + i, expected, got, mismatches - 1);
            }
        }
        for w in chunk.iter_mut() {
            *w = write;
        }
        bytes.fetch_add((chunk.len() * 16) as u64, Ordering::Relaxed);
    }
}

/// Each word stores its own index xor a per-pass golden; catches address-line faults.
fn address_in_address(
    buf: &mut [u64],
    seed: u64,
    sink: &ErrorSink,
    bytes: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
) {
    let golden = seed | 1;

    for (c, chunk) in buf.chunks_mut(CHUNK_WORDS).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let base = c * CHUNK_WORDS;
        for (i, w) in chunk.iter_mut().enumerate() {
            *w = (base + i) as u64 ^ golden;
        }
        bytes.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
    }
    std::hint::black_box(&*buf);

    for (c, chunk) in buf.chunks_mut(CHUNK_WORDS).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let base = c * CHUNK_WORDS;
        let mut mismatches: u64 = 0;
        for (i, w) in chunk.iter().enumerate() {
            mismatches += (*w != (base + i) as u64 ^ golden) as u64;
        }
        if mismatches > 0 {
            if let Some((i, got)) = chunk
                .iter()
                .enumerate()
                .find(|(i, w)| **w != (base + *i) as u64 ^ golden)
                .map(|(i, w)| (i, *w))
            {
                sink.record("addr-in-addr", base + i, (base + i) as u64 ^ golden, got, mismatches - 1);
            }
        }
        bytes.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
    }
}

/// Seeded xorshift sequence written then re-generated for verification.
fn random_pattern(
    buf: &mut [u64],
    seed: u64,
    sink: &ErrorSink,
    bytes: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
) {
    let mut rng = seed | 1;
    for chunk in buf.chunks_mut(CHUNK_WORDS) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        for w in chunk.iter_mut() {
            rng = xorshift64(rng);
            *w = rng;
        }
        bytes.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
    }
    std::hint::black_box(&*buf);

    let mut rng = seed | 1;
    for (c, chunk) in buf.chunks_mut(CHUNK_WORDS).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let base = c * CHUNK_WORDS;
        let mut mismatches: u64 = 0;
        let chunk_rng_start = rng;
        for w in chunk.iter() {
            rng = xorshift64(rng);
            mismatches += (*w != rng) as u64;
        }
        if mismatches > 0 {
            let mut r = chunk_rng_start;
            for (i, w) in chunk.iter().enumerate() {
                r = xorshift64(r);
                if *w != r {
                    sink.record("random", base + i, r, *w, mismatches - 1);
                    break;
                }
            }
        }
        bytes.fetch_add((chunk.len() * 8) as u64, Ordering::Relaxed);
    }
}

#[inline]
fn xorshift64(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> (ErrorSink, Arc<AtomicU64>, Arc<Mutex<Option<String>>>) {
        let counter = Arc::new(AtomicU64::new(0));
        let slot = Arc::new(Mutex::new(None));
        (
            ErrorSink {
                worker_id: 0,
                counter: counter.clone(),
                slot: slot.clone(),
            },
            counter,
            slot,
        )
    }

    #[test]
    fn sweep_catches_poisoned_word() {
        let (sink, counter, slot) = sink();
        let bytes = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));

        let pattern = 0x5555_5555_5555_5555u64;
        let mut buf = vec![pattern; CHUNK_WORDS * 2 + 17];
        buf[CHUNK_WORDS + 3] = pattern ^ (1 << 41);

        sweep(&mut buf, pattern, !pattern, false, &sink, &bytes, &cancel, "test");

        assert_eq!(counter.load(Ordering::Relaxed), 1);
        let msg = slot.lock().unwrap().clone().expect("error detail recorded");
        assert!(msg.contains("expected 0x5555555555555555"), "msg: {msg}");
        assert!(buf.iter().all(|w| *w == !pattern), "sweep must still rewrite");
    }

    #[test]
    fn sweep_clean_buffer_counts_nothing() {
        let (sink, counter, _slot) = sink();
        let bytes = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));

        let pattern = 0xAAAA_AAAA_AAAA_AAAAu64;
        let mut buf = vec![pattern; CHUNK_WORDS + 5];
        sweep(&mut buf, pattern, pattern, true, &sink, &bytes, &cancel, "test");
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }
}

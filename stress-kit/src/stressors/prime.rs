//! Prime-sieve stressor.
//!
//! Each worker runs a fresh Sieve-of-Eratosthenes pass over `SIEVE_LIMIT`,
//! integer-divides until completion, then starts over. Reports Mprime/s as
//! the count of primes *found* per second (deterministic per pass, so this
//! is a steady, repeatable signal).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const SIEVE_LIMIT: usize = 200_000;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let primes_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = primes_counter.clone();
            thread::Builder::new()
                .name("stress-kit-prime".into())
                .spawn(move || prime_worker(cancel, counter))
                .expect("stress-kit: failed to spawn prime worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = primes_counter.load(Ordering::Relaxed);
            let delta = now.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mprime = (delta as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mprime,
                last_error: None,
                fatal: false,
            });

            last_count = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn prime_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut sieve = vec![true; SIEVE_LIMIT + 1];
    while !cancel.load(Ordering::Relaxed) {
        // Reset.
        for b in sieve.iter_mut() {
            *b = true;
        }
        sieve[0] = false;
        sieve[1] = false;

        let mut i = 2usize;
        while i * i <= SIEVE_LIMIT {
            if sieve[i] {
                let mut j = i * i;
                while j <= SIEVE_LIMIT {
                    sieve[j] = false;
                    j += i;
                }
            }
            i += 1;
            if cancel.load(Ordering::Relaxed) {
                return;
            }
        }

        let found: u64 = sieve.iter().filter(|b| **b).count() as u64;
        std::hint::black_box(&sieve);
        counter.fetch_add(found, Ordering::Relaxed);
    }
}

//! Linpack-style solver stress: per worker, repeatedly factor a random NxN
//! system with partially-pivoted LU, solve, and check the normalized
//! residual ||Ax-b||inf / (||A||inf * ||x||inf * N * eps) against the HPL
//! convention threshold. Breaches accumulate in `Metrics::errors`.
//! Workers advance a shared flop counter per eliminated column, so every
//! tick reports GFLOPS even while a solve is still in flight.
//!
//! A is regenerated from its seed for the residual check, so each worker
//! holds one matrix plus three vectors.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const TICK: Duration = Duration::from_millis(500);
const RESIDUAL_THRESHOLD: f64 = 16.0;
const MIN_N: usize = 256;
const MAX_N: usize = 2048;

pub(crate) fn run(
    thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let n = matrix_order(memory_cap_mb, thread_count);

    let flop_counter = Arc::new(AtomicU64::new(0));
    let error_counter = Arc::new(AtomicU64::new(0));
    let error_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let handles: Vec<_> = (0..thread_count)
        .map(|worker_id| {
            let cancel = cancel.clone();
            let flops = flop_counter.clone();
            let errors = error_counter.clone();
            let slot = error_slot.clone();
            thread::Builder::new()
                .name("stress-kit-linpack".into())
                .spawn(move || linpack_worker(worker_id, n, cancel, flops, errors, slot))
                .expect("stress-kit: failed to spawn linpack worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_flops: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = flop_counter.load(Ordering::Relaxed);
            let delta = now.saturating_sub(last_flops);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let gflops = delta as f64 / delta_secs / 1e9;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: gflops,
                last_error: error_slot.lock().ok().and_then(|g| g.clone()),
                fatal: false,
                errors: error_counter.load(Ordering::Relaxed),
            });

            last_flops = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

/// Largest N whose matrix + vectors fit in ~90% of this worker's share.
fn matrix_order(memory_cap_mb: u64, threads: usize) -> usize {
    let per_worker_bytes = (memory_cap_mb.max(1) * 1024 * 1024) / threads.max(1) as u64;
    let n = (((per_worker_bytes as f64 * 0.9) / 8.0).sqrt()) as usize;
    n.clamp(MIN_N, MAX_N)
}

fn linpack_worker(
    worker_id: usize,
    n: usize,
    cancel: Arc<AtomicBool>,
    flops: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    slot: Arc<Mutex<Option<String>>>,
) {
    let mut a = vec![0.0f64; n * n];
    let mut b = vec![0.0f64; n];
    let mut x = vec![0.0f64; n];

    let mut pass: u64 = 0;
    'outer: while !cancel.load(Ordering::Relaxed) {
        let seed = 0xD1B5_4A32_D192_ED03u64 ^ ((worker_id as u64) << 40) ^ pass;
        pass = pass.wrapping_add(1);

        // b = A * ones, so the exact solution is all ones.
        let mut rng = seed | 1;
        for row in 0..n {
            let mut sum = 0.0;
            for col in 0..n {
                rng = xorshift64(rng);
                let v = uniform(rng);
                a[row * n + col] = v;
                sum += v;
            }
            b[row] = sum;
        }
        std::hint::black_box(&a);

        // In-place LU with partial pivoting.
        for k in 0..n {
            if k % 64 == 0 && cancel.load(Ordering::Relaxed) {
                return;
            }
            let mut max_val = a[k * n + k].abs();
            let mut max_row = k;
            for row in (k + 1)..n {
                let v = a[row * n + k].abs();
                if v > max_val {
                    max_val = v;
                    max_row = row;
                }
            }
            if max_val < 1e-300 {
                // Numerically singular draw — not a hardware fault; reroll.
                continue 'outer;
            }
            if max_row != k {
                for col in 0..n {
                    a.swap(k * n + col, max_row * n + col);
                }
                b.swap(k, max_row);
            }

            let pivot = a[k * n + k];
            for row in (k + 1)..n {
                let factor = a[row * n + k] / pivot;
                a[row * n + k] = factor;
                let (upper, lower) = a.split_at_mut(row * n);
                let src = &upper[k * n + k + 1..k * n + n];
                let dst = &mut lower[k + 1..n];
                for (d, s) in dst.iter_mut().zip(src) {
                    *d -= factor * *s;
                }
                b[row] -= factor * b[k];
            }
            // Column k: (n-k-1) rows × (1 div + 2(n-k-1) row update + 2 b update).
            let rows = (n - k - 1) as u64;
            flops.fetch_add(rows * (2 * rows + 3), Ordering::Relaxed);
        }

        // Back substitution into x.
        for row in (0..n).rev() {
            let mut sum = b[row];
            for col in (row + 1)..n {
                sum -= a[row * n + col] * x[col];
            }
            x[row] = sum / a[row * n + row];
        }
        std::hint::black_box(&x);
        // Back substitution: ~n^2 flops.
        flops.fetch_add((n * n) as u64, Ordering::Relaxed);

        // Residual against the regenerated original system.
        let mut rng = seed | 1;
        let mut r_inf = 0.0f64;
        let mut a_inf = 0.0f64;
        for _row in 0..n {
            let mut ax = 0.0;
            let mut row_sum = 0.0;
            let mut row_abs = 0.0;
            for col in 0..n {
                rng = xorshift64(rng);
                let v = uniform(rng);
                ax += v * x[col];
                row_sum += v;
                row_abs += v.abs();
            }
            r_inf = r_inf.max((ax - row_sum).abs());
            a_inf = a_inf.max(row_abs);
        }
        let x_inf = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let denom = a_inf * x_inf * n as f64 * f64::EPSILON;
        let normalized = if denom > 0.0 { r_inf / denom } else { f64::INFINITY };

        if !normalized.is_finite() || normalized > RESIDUAL_THRESHOLD {
            errors.fetch_add(1, Ordering::Relaxed);
            let msg = format!(
                "linpack[{worker_id}]: residual {normalized:.2} exceeds {RESIDUAL_THRESHOLD} (N={n} seed 0x{seed:016X})"
            );
            log::error!("[stress-kit/linpack] {msg}");
            if let Ok(mut g) = slot.lock() {
                *g = Some(msg);
            }
        }
    }
}

#[inline]
fn xorshift64(mut v: u64) -> u64 {
    v ^= v << 13;
    v ^= v >> 7;
    v ^= v << 17;
    v
}

/// Map a u64 to [-0.5, 0.5).
#[inline]
fn uniform(v: u64) -> f64 {
    (v >> 11) as f64 * (1.0 / (1u64 << 53) as f64) - 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Benchmark-shaped run (all threads, 1024 MB cap) must produce nonzero
    /// throughput ticks while the first solve is still in flight.
    #[test]
    fn ticks_report_throughput_mid_solve() {
        let thread_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Metrics>();
        let started = Instant::now();

        let cancel_run = cancel.clone();
        let supervisor = thread::spawn(move || {
            run(thread_count, 1024, &cancel_run, &tx, started);
        });

        let mut nonzero = 0u32;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && nonzero < 3 {
            match rx.recv_timeout(Duration::from_millis(600)) {
                Ok(m) if m.throughput > 0.0 => nonzero += 1,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        cancel.store(true, Ordering::SeqCst);
        let _ = supervisor.join();

        assert!(
            nonzero >= 3,
            "expected >=3 nonzero throughput ticks within 10s, got {nonzero}"
        );
    }
}

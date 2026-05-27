//! Square `f32` matmul per worker; reports Mflop/s based on the standard
//! `2*N^3` flops-per-matmul accounting.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const N: usize = 256;
const FLOPS_PER_MATMUL: u64 = 2 * (N as u64) * (N as u64) * (N as u64);
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let matmul_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = matmul_counter.clone();
            thread::Builder::new()
                .name("stress-kit-matrix".into())
                .spawn(move || matrix_worker(cancel, counter))
                .expect("stress-kit: failed to spawn matrix worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_count = matmul_counter.load(Ordering::Relaxed);
            let delta = now_count.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mflops = (delta as f64 * FLOPS_PER_MATMUL as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mflops,
                last_error: None,
                fatal: false,
            });

            last_count = now_count;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn matrix_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut a = vec![0.0_f32; N * N];
    let mut b = vec![0.0_f32; N * N];
    let mut c = vec![0.0_f32; N * N];

    for i in 0..N * N {
        a[i] = ((i % 7) as f32) * 0.5 + 1.0;
        b[i] = ((i % 11) as f32) * 0.25 + 1.0;
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        matmul(&a, &b, &mut c);
        // Fold result back to keep the optimizer honest.
        a[0] = c[0] * 0.0 + 1.0;
        std::hint::black_box(&c);
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline(always)]
fn matmul(a: &[f32], b: &[f32], c: &mut [f32]) {
    // Plain ijk; the optimizer auto-vectorizes the inner sum on release builds.
    for i in 0..N {
        for k in 0..N {
            let aik = a[i * N + k];
            let row_b = &b[k * N..k * N + N];
            let row_c = &mut c[i * N..i * N + N];
            for j in 0..N {
                row_c[j] += aik * row_b[j];
            }
        }
    }
}

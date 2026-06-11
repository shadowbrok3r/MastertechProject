//! Duplicate-execution CPU stability test. Runs a deterministic mixed
//! integer + floating-point workload twice per seed and compares digests;
//! any divergence is silent data corruption and accumulates in
//! `Metrics::errors`. Reports Mop/s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const TICK: Duration = Duration::from_millis(500);
const LANES: usize = 32;
const ROUNDS: usize = 64;
const CALLS_PER_BURST: u64 = 32;
/// Integer ops per round per lane (~4) plus the FP mix tail per round.
const OPS_PER_CALL: u64 = (ROUNDS * LANES * 4 + ROUNDS * 8) as u64;
/// Two executions per verified call.
const OPS_PER_BURST: u64 = CALLS_PER_BURST * OPS_PER_CALL * 2;

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let burst_counter = Arc::new(AtomicU64::new(0));
    let error_counter = Arc::new(AtomicU64::new(0));
    let error_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let handles: Vec<_> = (0..thread_count)
        .map(|worker_id| {
            let cancel = cancel.clone();
            let counter = burst_counter.clone();
            let errors = error_counter.clone();
            let slot = error_slot.clone();
            thread::Builder::new()
                .name("stress-kit-cpu-verify".into())
                .spawn(move || verify_worker(worker_id, cancel, counter, errors, slot))
                .expect("stress-kit: failed to spawn cpu-verify worker")
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
            let mops = (delta as f64 * OPS_PER_BURST as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mops,
                last_error: error_slot.lock().ok().and_then(|g| g.clone()),
                fatal: false,
                errors: error_counter.load(Ordering::Relaxed),
            });

            last_count = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn verify_worker(
    worker_id: usize,
    cancel: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    slot: Arc<Mutex<Option<String>>>,
) {
    let mut pass: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..CALLS_PER_BURST {
            let seed = 0xA076_1D64_78BD_642Fu64
                ^ ((worker_id as u64) << 48)
                ^ pass.wrapping_mul(0x2545_F491_4F6C_DD1D);
            pass = pass.wrapping_add(1);

            // black_box forces two independent executions; without it the
            // optimizer would CSE the identical pure calls into one.
            let d1 = workload(std::hint::black_box(seed));
            let d2 = workload(std::hint::black_box(seed));

            if d1 != d2 {
                errors.fetch_add(1, Ordering::Relaxed);
                let msg = format!(
                    "cpu-verify[{worker_id}]: digest mismatch 0x{d1:016X} != 0x{d2:016X} (seed 0x{seed:016X})"
                );
                log::error!("[stress-kit/cpu-verify] {msg}");
                if let Ok(mut g) = slot.lock() {
                    *g = Some(msg);
                }
            }
        }
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// Deterministic mixed workload: integer mul/rotate/xor lanes plus an FMA /
/// sqrt chain, folded into a single digest.
fn workload(seed: u64) -> u64 {
    let mut lanes = [0u64; LANES];
    let mut s = seed | 1;
    for lane in lanes.iter_mut() {
        s = s.wrapping_mul(0x5851_F42D_4C95_7F2D).wrapping_add(0x14057B7EF767814F);
        *lane = s;
    }

    let mut facc = 1.000_000_1f64;
    let mut fsum = 0.0f64;

    for round in 0..ROUNDS {
        let r = round as u64;
        for i in 0..LANES {
            let prev = lanes[(i + LANES - 1) % LANES];
            let mut v = lanes[i];
            v = v.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            v ^= prev >> 29;
            v = v.rotate_left(((prev ^ r) & 63) as u32);
            v = v.wrapping_add(prev ^ 0xBF58_476D_1CE4_E5B9);
            lanes[i] = v;
        }

        let x = 1.0 + (lanes[round % LANES] >> 12) as f64 * (1.0 / (1u64 << 52) as f64);
        facc = facc.mul_add(1.000_000_001, x.sqrt() * 1e-3);
        fsum += facc;
        if !facc.is_finite() || facc > 1e12 {
            facc = 1.000_000_1;
        }
    }

    let mut digest = 0xCBF2_9CE4_8422_2325u64;
    for v in lanes {
        digest ^= v;
        digest = digest.wrapping_mul(0x0000_0100_0000_01B3);
    }
    digest ^= facc.to_bits();
    digest = digest.wrapping_mul(0x0000_0100_0000_01B3);
    digest ^= fsum.to_bits();
    digest
}

//! Verified AVX2/FMA3 modular multiplies on a cache-resident working set; reports GFLOPS.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;
use crate::lowlevel::affinity;

use super::modmul::{self, BLOCK, ErrorLog, FLOPS_PER_MUL, Lane4, Pass, Path};

const TICK: Duration = Duration::from_millis(500);
/// 64 KiB of residues per worker.
const WORKER_VECTORS: usize = 64 * 1024 / std::mem::size_of::<Lane4>();
/// Multiply rounds per block between checks.
const ROUNDS: u32 = 64;
const NO_AVX2: &str = "avx2: inconclusive - this CPU has no AVX2/FMA3, so the AVX2 path never ran";

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    if !modmul::avx2_available() {
        log::warn!("[stress-kit/avx2] {NO_AVX2}");
        let _ = tx.send(Metrics {
            elapsed_secs: started_at.elapsed().as_secs_f64(),
            throughput: 0.0,
            last_error: Some(NO_AVX2.to_string()),
            fatal: true,
            errors: 0,
        });
        return;
    }

    let muls = Arc::new(AtomicU64::new(0));
    let log = Arc::new(ErrorLog::new("avx2", "cache"));

    let handles: Vec<_> = (0..thread_count)
        .map(|worker_id| {
            let cancel = cancel.clone();
            let muls = muls.clone();
            let log = log.clone();
            thread::Builder::new()
                .name("stress-kit-avx2".into())
                .spawn(move || avx2_worker(worker_id, cancel, muls, log))
                .expect("stress-kit: failed to spawn avx2 worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_muls: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = muls.load(Ordering::Relaxed);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let gflops = now.saturating_sub(last_muls) as f64 * FLOPS_PER_MUL / delta_secs / 1e9;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: gflops,
                last_error: log.last(),
                fatal: false,
                errors: log.count(),
            });

            last_muls = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn avx2_worker(
    worker_id: usize,
    cancel: Arc<AtomicBool>,
    muls: Arc<AtomicU64>,
    log: Arc<ErrorLog>,
) {
    let mut rng = 0xC2B2_AE3D_27D4_EB4F ^ ((worker_id as u64 + 1) << 40);
    let mut pass = Pass::new(modmul::residue(&mut rng), modmul::residue(&mut rng), ROUNDS);
    let mut buf = vec![Lane4::default(); WORKER_VECTORS];
    modmul::fill(Path::Avx2, &mut buf, 0, pass.g_in);
    let mut faults = Vec::new();

    while !cancel.load(Ordering::Relaxed) {
        for (b, block) in buf.chunks_mut(BLOCK).enumerate() {
            modmul::run_pass(
                Path::Avx2,
                block,
                (b * BLOCK * 4) as u64,
                &pass,
                &mut faults,
            );
            if !faults.is_empty() {
                log.record(worker_id, affinity::current(), &faults);
                faults.clear();
            }
        }
        muls.fetch_add(pass.mul_count(WORKER_VECTORS as u64 * 4), Ordering::Relaxed);
        pass = pass.then(modmul::residue(&mut rng));
    }
}

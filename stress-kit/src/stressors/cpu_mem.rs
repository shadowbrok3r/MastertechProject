//! OCCT-style CPU + memory test: verified multiplies streamed through RAM; reports MiB/s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;
use crate::lowlevel::affinity;

use super::modmul::{self, BLOCK, ErrorLog, Lane4, Pass, Path};

const TICK: Duration = Duration::from_millis(500);
const MIN_WORKER_MB: u64 = 16;
/// Multiply rounds per lane per pass.
const ROUNDS: u32 = 16;
/// Blocks between cancel checks and throughput updates.
const BLOCKS_PER_STEP: usize = 64;
const VECTOR_BYTES: u64 = std::mem::size_of::<Lane4>() as u64;

pub(crate) fn run(
    thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let path = Path::detect();
    let cap_per_thread_mb = (memory_cap_mb / thread_count.max(1) as u64).max(MIN_WORKER_MB);
    log::info!(
        "[stress-kit/cpu_mem] {} path, {thread_count} worker(s) x {cap_per_thread_mb} MiB",
        path.label()
    );
    let bytes = Arc::new(AtomicU64::new(0));
    let log = Arc::new(ErrorLog::new("cpu_mem", "memory"));

    let handles: Vec<_> = (0..thread_count)
        .map(|worker_id| {
            let cancel = cancel.clone();
            let bytes = bytes.clone();
            let log = log.clone();
            thread::Builder::new()
                .name("stress-kit-cpu-mem".into())
                .spawn(move || {
                    cpu_mem_worker(worker_id, path, cap_per_thread_mb, cancel, bytes, log)
                })
                .expect("stress-kit: failed to spawn cpu_mem worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_bytes: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = bytes.load(Ordering::Relaxed);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mib_per_sec =
                now.saturating_sub(last_bytes) as f64 / (1024.0 * 1024.0) / delta_secs;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error: log.last(),
                fatal: false,
                errors: log.count(),
            });

            last_bytes = now;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

fn cpu_mem_worker(
    worker_id: usize,
    path: Path,
    cap_mb: u64,
    cancel: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
    log: Arc<ErrorLog>,
) {
    // Halves the request until an allocation succeeds.
    let mut vectors = (cap_mb * 1024 * 1024 / VECTOR_BYTES) as usize;
    let min_vectors = (MIN_WORKER_MB * 1024 * 1024 / VECTOR_BYTES) as usize;
    let mut buf: Vec<Lane4> = loop {
        let mut v: Vec<Lane4> = Vec::new();
        if v.try_reserve_exact(vectors).is_ok() {
            v.resize(vectors, Lane4::default());
            break v;
        }
        if vectors <= min_vectors {
            log::warn!("[stress-kit/cpu_mem] worker {worker_id}: allocation failed, exiting");
            return;
        }
        vectors /= 2;
    };

    let mut rng = 0xD6E8_FEB8_6659_FD93 ^ ((worker_id as u64 + 1) << 40);
    let mut pass = Pass::new(modmul::residue(&mut rng), modmul::residue(&mut rng), ROUNDS);
    for (b, block) in buf.chunks_mut(BLOCK).enumerate() {
        if b % BLOCKS_PER_STEP == 0 && cancel.load(Ordering::Relaxed) {
            return;
        }
        modmul::fill(path, block, (b * BLOCK * 4) as u64, pass.g_in);
    }
    bytes.fetch_add(buf.len() as u64 * VECTOR_BYTES, Ordering::Relaxed);

    let mut faults = Vec::new();
    loop {
        for (step, blocks) in buf.chunks_mut(BLOCK * BLOCKS_PER_STEP).enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let first = step * BLOCK * BLOCKS_PER_STEP;
            for (b, block) in blocks.chunks_mut(BLOCK).enumerate() {
                let base = ((first + b * BLOCK) * 4) as u64;
                modmul::run_pass(path, block, base, &pass, &mut faults);
                if !faults.is_empty() {
                    log.record(worker_id, affinity::current(), &faults);
                    faults.clear();
                }
            }
            bytes.fetch_add(2 * blocks.len() as u64 * VECTOR_BYTES, Ordering::Relaxed);
        }
        pass = pass.then(modmul::residue(&mut rng));
    }
}

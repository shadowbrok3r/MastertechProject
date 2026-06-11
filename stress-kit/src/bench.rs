//! One-shot memory-hierarchy measurements: pointer-chase latency and
//! sequential read bandwidth across working-set sizes (L1 → RAM). These are
//! benchmarks, not stressors — call them directly, no session required.

use std::time::Instant;

use serde::{Deserialize, Serialize};

/// One working-set sample of the ladder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LadderPoint {
    pub size_kb: u32,
    /// Dependent-load (pointer-chase) latency.
    pub latency_ns: f64,
    /// Sequential u64 read bandwidth.
    pub read_gb_per_s: f64,
}

/// Default ladder: 4 KiB → 128 MiB, doubling.
pub fn default_ladder_sizes_kb() -> Vec<u32> {
    let mut v = Vec::new();
    let mut kb = 4u32;
    while kb <= 128 * 1024 {
        v.push(kb);
        kb *= 2;
    }
    v
}

/// Measure latency + bandwidth at each working-set size. Single-threaded;
/// the full default ladder takes a few seconds.
pub fn measure_ladder(sizes_kb: &[u32]) -> Vec<LadderPoint> {
    sizes_kb
        .iter()
        .filter(|kb| **kb >= 1)
        .map(|&size_kb| LadderPoint {
            size_kb,
            latency_ns: chase_latency_ns(size_kb as usize * 1024),
            read_gb_per_s: read_bandwidth_gbps(size_kb as usize * 1024),
        })
        .collect()
}

/// Random-cycle pointer chase: every load depends on the previous one, so
/// elapsed/steps is the average load-to-use latency at this footprint.
fn chase_latency_ns(size_bytes: usize) -> f64 {
    let n = (size_bytes / std::mem::size_of::<usize>()).max(64);
    let mut next: Vec<usize> = (0..n).collect();
    sattolo_shuffle(&mut next, 0x9E37_79B9_7F4A_7C15 ^ n as u64);

    // Warm pass faults pages and primes the cache level under test.
    let mut p = 0usize;
    for _ in 0..n {
        p = next[p];
    }
    std::hint::black_box(p);

    let steps = (n * 4).clamp(1 << 18, 1 << 23);
    let start = Instant::now();
    let mut p = 0usize;
    for _ in 0..steps {
        p = next[p];
    }
    std::hint::black_box(p);
    start.elapsed().as_nanos() as f64 / steps as f64
}

/// Repeated sequential u64 sum over the buffer until ~60 ms elapses.
fn read_bandwidth_gbps(size_bytes: usize) -> f64 {
    let n = (size_bytes / 8).max(1024);
    let buf: Vec<u64> = (0..n as u64).map(|i| i.wrapping_mul(0x9E37_79B9)).collect();

    let mut bytes: u64 = 0;
    let mut acc: u64 = 0;
    let start = Instant::now();
    while start.elapsed().as_millis() < 60 {
        for w in &buf {
            acc = acc.wrapping_add(*w);
        }
        bytes += (n * 8) as u64;
    }
    std::hint::black_box(acc);
    let secs = start.elapsed().as_secs_f64().max(f64::EPSILON);
    bytes as f64 / secs / 1e9
}

/// Random cycle visiting every element exactly once (one big loop).
fn sattolo_shuffle(v: &mut [usize], mut seed: u64) {
    for i in (1..v.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % i as u64) as usize;
        v.swap(i, j);
    }
}

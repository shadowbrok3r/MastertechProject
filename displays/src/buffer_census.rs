//! Named gauges for every queue whose producer outruns its consumer.
//!
//! A queue registers a [`QueueStats`] and publishes its depth, byte size and
//! drop counters on every push. [`snapshot`] reads them all back, so an
//! unbounded-growth report names the buffer instead of guessing at it.

use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// Live counters for one bounded queue.
#[derive(Debug)]
pub struct QueueStats {
    label: String,
    cap_frames: usize,
    cap_bytes: usize,
    depth: AtomicUsize,
    bytes: AtomicUsize,
    peak_depth: AtomicUsize,
    peak_bytes: AtomicUsize,
    dropped: AtomicU64,
    dropped_bytes: AtomicU64,
}

impl QueueStats {
    /// Registers a new gauge and returns the handle its owner publishes into.
    pub fn register(label: impl Into<String>, cap_frames: usize, cap_bytes: usize) -> Arc<Self> {
        let stats = Arc::new(Self {
            label: label.into(),
            cap_frames,
            cap_bytes,
            depth: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            peak_depth: AtomicUsize::new(0),
            peak_bytes: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
            dropped_bytes: AtomicU64::new(0),
        });
        let mut reg = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        reg.retain(|w| w.strong_count() > 0);
        reg.push(Arc::downgrade(&stats));
        stats
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn cap_frames(&self) -> usize {
        self.cap_frames
    }

    pub fn cap_bytes(&self) -> usize {
        self.cap_bytes
    }

    /// Records the queue's current occupancy, tracking the high-water mark.
    pub fn set_occupancy(&self, depth: usize, bytes: usize) {
        self.depth.store(depth, Ordering::Relaxed);
        self.bytes.store(bytes, Ordering::Relaxed);
        self.peak_depth.fetch_max(depth, Ordering::Relaxed);
        self.peak_bytes.fetch_max(bytes, Ordering::Relaxed);
    }

    /// Records one dropped entry of `bytes`.
    pub fn record_drop(&self, bytes: usize) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
        self.dropped_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn depth(&self) -> usize {
        self.depth.load(Ordering::Relaxed)
    }

    pub fn bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes.load(Ordering::Relaxed)
    }

    fn snapshot(&self) -> GaugeSnapshot {
        GaugeSnapshot {
            label: self.label.clone(),
            depth: self.depth(),
            bytes: self.bytes(),
            peak_depth: self.peak_depth.load(Ordering::Relaxed),
            peak_bytes: self.peak_bytes.load(Ordering::Relaxed),
            dropped: self.dropped(),
            dropped_bytes: self.dropped_bytes(),
            cap_frames: self.cap_frames,
            cap_bytes: self.cap_bytes,
        }
    }
}

/// One gauge's values at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaugeSnapshot {
    pub label: String,
    pub depth: usize,
    pub bytes: usize,
    pub peak_depth: usize,
    pub peak_bytes: usize,
    pub dropped: u64,
    pub dropped_bytes: u64,
    pub cap_frames: usize,
    pub cap_bytes: usize,
}

impl GaugeSnapshot {
    /// Fraction of the tighter of the two caps that is currently occupied.
    pub fn fill_ratio(&self) -> f32 {
        let by_frames = ratio(self.depth, self.cap_frames);
        let by_bytes = ratio(self.bytes, self.cap_bytes);
        by_frames.max(by_bytes)
    }
}

fn ratio(used: usize, cap: usize) -> f32 {
    if cap == 0 {
        0.0
    } else {
        used as f32 / cap as f32
    }
}

static REGISTRY: Lazy<Mutex<Vec<Weak<QueueStats>>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Reads every live gauge, dropping registrations whose owner is gone.
pub fn snapshot() -> Vec<GaugeSnapshot> {
    let mut reg = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    reg.retain(|w| w.strong_count() > 0);
    reg.iter()
        .filter_map(|w| w.upgrade())
        .map(|s| s.snapshot())
        .collect()
}

/// Total bytes held across every registered queue.
pub fn total_bytes() -> usize {
    snapshot().iter().map(|g| g.bytes).sum()
}

/// Logs any gauge that has dropped entries or is over `warn_ratio` full.
pub fn log_notable(warn_ratio: f32) {
    for g in snapshot() {
        if g.dropped == 0 && g.fill_ratio() < warn_ratio {
            continue;
        }
        log::warn!(
            target: "buffer_census",
            "{}: {} frames / {} bytes (peak {} / {}), dropped {} frames / {} bytes, caps {} / {}",
            g.label,
            g.depth,
            g.bytes,
            g.peak_depth,
            g.peak_bytes,
            g.dropped,
            g.dropped_bytes,
            g.cap_frames,
            g.cap_bytes
        );
    }
}

/// Shortest gap between two throttled census logs.
const LOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Gauge occupancy that counts as notable on its own.
const WARN_RATIO: f32 = 0.5;

static LAST_LOG: Lazy<Mutex<Option<web_time::Instant>>> = Lazy::new(|| Mutex::new(None));

/// Calls [`log_notable`] at most once per [`LOG_INTERVAL`].
pub fn log_notable_throttled() {
    {
        let mut last = LAST_LOG.lock().unwrap_or_else(|e| e.into_inner());
        match *last {
            Some(at) if at.elapsed() < LOG_INTERVAL => return,
            _ => *last = Some(web_time::Instant::now()),
        }
    }
    log_notable(WARN_RATIO);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_publishes_and_appears_in_snapshot() {
        let stats = QueueStats::register("test.gauge.publish", 10, 1000);
        stats.set_occupancy(3, 300);
        stats.record_drop(50);

        let found = snapshot()
            .into_iter()
            .find(|g| g.label == "test.gauge.publish")
            .expect("registered gauge is visible in the snapshot");
        assert_eq!(found.depth, 3);
        assert_eq!(found.bytes, 300);
        assert_eq!(found.dropped, 1);
        assert_eq!(found.dropped_bytes, 50);
    }

    #[test]
    fn peak_survives_a_drained_queue() {
        let stats = QueueStats::register("test.gauge.peak", 10, 1000);
        stats.set_occupancy(7, 700);
        stats.set_occupancy(0, 0);

        let found = snapshot()
            .into_iter()
            .find(|g| g.label == "test.gauge.peak")
            .expect("gauge present");
        assert_eq!(found.depth, 0);
        assert_eq!(found.peak_depth, 7);
        assert_eq!(found.peak_bytes, 700);
    }

    #[test]
    fn dropped_registration_leaves_the_registry() {
        let stats = QueueStats::register("test.gauge.transient", 4, 400);
        assert!(snapshot().iter().any(|g| g.label == "test.gauge.transient"));
        drop(stats);
        assert!(!snapshot().iter().any(|g| g.label == "test.gauge.transient"));
    }

    #[test]
    fn fill_ratio_takes_the_tighter_cap() {
        let stats = QueueStats::register("test.gauge.ratio", 100, 100);
        stats.set_occupancy(10, 90);
        let found = snapshot()
            .into_iter()
            .find(|g| g.label == "test.gauge.ratio")
            .expect("gauge present");
        assert!((found.fill_ratio() - 0.9).abs() < f32::EPSILON);
    }
}

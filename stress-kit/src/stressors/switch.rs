//! Context-switch stressor.
//!
//! Workers come in pairs that ping-pong on a pair of `Condvar`s. Every
//! handoff is one OS context switch on the scheduler. Reports Mctxsw/s
//! across all pairs.
//!
//! If `thread_count` is odd we round down to the next even number; a single
//! worker can't ping-pong with itself.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let pairs = (thread_count / 2).max(1);
    let switches = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::with_capacity(pairs * 2);
    for _ in 0..pairs {
        let slot = Arc::new((Mutex::new(0u8), Condvar::new()));
        // 0 = A's turn, 1 = B's turn.

        for who in 0u8..2 {
            let slot = slot.clone();
            let cancel = cancel.clone();
            let switches = switches.clone();
            let h = thread::Builder::new()
                .name("stress-kit-switch".into())
                .spawn(move || switch_worker(who, slot, cancel, switches))
                .expect("stress-kit: failed to spawn switch worker");
            handles.push(h);
        }
    }

    let mut last_tick = Instant::now();
    let mut last_count: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now = switches.load(Ordering::Relaxed);
            let delta = now.saturating_sub(last_count);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mctxsw = (delta as f64) / delta_secs / 1e6;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mctxsw,
                last_error: None,
                fatal: false,
                errors: 0,
            });

            last_count = now;
            last_tick = Instant::now();
        }
    }

    // Wake any sleepers so they observe `cancel` and unwind.
    // The handles will drop the Arc, so the slot goes with them.
    for h in handles {
        let _ = h.join();
    }
}

fn switch_worker(
    who: u8,
    slot: Arc<(Mutex<u8>, Condvar)>,
    cancel: Arc<AtomicBool>,
    switches: Arc<AtomicU64>,
) {
    let (lock, cv) = &*slot;
    while !cancel.load(Ordering::Relaxed) {
        let mut g = match lock.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        // Wait until it's our turn or we're cancelled.
        loop {
            if *g == who {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let r = cv
                .wait_timeout(g, Duration::from_millis(50))
                .map(|(g, _)| g);
            g = match r {
                Ok(g) => g,
                Err(_) => return,
            };
        }
        // Flip the turn and notify the other side.
        *g = who ^ 1;
        switches.fetch_add(1, Ordering::Relaxed);
        cv.notify_all();
    }
    // Make sure the partner doesn't get stuck waiting on a turn we'll never give.
    if let Ok(mut g) = lock.lock() {
        *g = who ^ 1;
    }
    cv.notify_all();
}

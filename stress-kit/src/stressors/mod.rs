pub mod cpu;
pub mod disk;
pub mod memory;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use crate::{Metrics, StressConfig, Stressor};

/// Used by [`crate::StressSession`]: optional timeout thread sets `cancel`, then [`run_core`].
pub(crate) fn run(
    config: StressConfig,
    thread_count: usize,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    if let Some(timeout) = config.timeout {
        let cancel_wdog = cancel.clone();
        std::thread::Builder::new()
            .name("stress-kit-watchdog".into())
            .spawn(move || {
                std::thread::sleep(timeout);
                cancel_wdog.store(true, Ordering::SeqCst);
                log::debug!("stress-kit: timeout watchdog fired after {:?}", timeout);
            })
            .expect("stress-kit: failed to spawn watchdog thread");
    }

    run_core(&config, thread_count, &cancel, &tx, started_at);

    log::debug!("stress-kit: supervisor exiting");
}

/// Same workers as [`run`] but no timeout thread; caller owns `cancel` (scenario supervisor).
pub(crate) fn run_core(
    config: &StressConfig,
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    match config.stressor {
        Stressor::Cpu => cpu::run(thread_count, cancel, tx, started_at),
        Stressor::Memory => {
            memory::run(thread_count, config.memory_cap_mb, cancel, tx, started_at)
        }
        Stressor::Disk => disk::run(thread_count, config.disk_file_mb, cancel, tx, started_at),
    }
}

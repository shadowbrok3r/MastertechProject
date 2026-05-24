pub mod atomic;
pub mod bitops;
pub mod branch;
pub mod cache;
pub mod cpu;
pub mod disk;
pub mod fp;
pub mod hash;
pub mod icache;
pub mod matrix;
pub mod memcpy;
pub mod memory;
pub mod mutex;
pub mod prefetch;
pub mod prime;
pub mod stream;
pub mod switch;
pub mod tsc;
pub mod vm;

#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
pub mod gpu_matmul;
#[cfg(feature = "gpu")]
pub mod gpu_vram;
#[cfg(feature = "gpu")]
pub mod gpu_pcie;
#[cfg(feature = "gpu")]
mod gpu_common;

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
        Stressor::Matrix => matrix::run(thread_count, cancel, tx, started_at),
        Stressor::Memcpy => memcpy::run(thread_count, cancel, tx, started_at),
        Stressor::Bitops => bitops::run(thread_count, cancel, tx, started_at),
        Stressor::Cache => cache::run(thread_count, cancel, tx, started_at),
        Stressor::Vm => vm::run(thread_count, config.memory_cap_mb, cancel, tx, started_at),
        Stressor::Stream => stream::run(thread_count, cancel, tx, started_at),
        Stressor::Branch => branch::run(thread_count, cancel, tx, started_at),
        Stressor::Atomic => atomic::run(thread_count, cancel, tx, started_at),
        Stressor::Mutex => mutex::run(thread_count, cancel, tx, started_at),
        Stressor::Switch => switch::run(thread_count, cancel, tx, started_at),
        Stressor::Prime => prime::run(thread_count, cancel, tx, started_at),
        Stressor::Fp => fp::run(thread_count, cancel, tx, started_at),
        Stressor::Hash => hash::run(thread_count, cancel, tx, started_at),
        Stressor::Prefetch => prefetch::run(thread_count, cancel, tx, started_at),
        Stressor::Icache => icache::run(thread_count, cancel, tx, started_at),
        Stressor::Tsc => tsc::run(thread_count, cancel, tx, started_at),

        #[cfg(feature = "gpu")]
        Stressor::Gpu => gpu::run(thread_count, cancel, tx, started_at),
        #[cfg(feature = "gpu")]
        Stressor::GpuMatmul => gpu_matmul::run(thread_count, cancel, tx, started_at),
        #[cfg(feature = "gpu")]
        Stressor::GpuVram => gpu_vram::run(thread_count, config.memory_cap_mb, cancel, tx, started_at),
        #[cfg(feature = "gpu")]
        Stressor::GpuPcie => gpu_pcie::run(thread_count, config.memory_cap_mb, cancel, tx, started_at),

        #[cfg(not(feature = "gpu"))]
        Stressor::Gpu | Stressor::GpuMatmul | Stressor::GpuVram | Stressor::GpuPcie => {
            log::warn!("stress-kit built without 'gpu' feature; GPU stressor dispatched as no-op");
            let _ = (config, thread_count);
            let _ = tx.send(crate::Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: 0.0,
                last_error: Some("stress-kit built without 'gpu' feature".into()),
            });
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

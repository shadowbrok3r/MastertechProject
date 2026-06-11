//! VM / page-pressure stressor.
//!
//! On Windows: `VirtualAlloc` a `MEM_COMMIT | MEM_RESERVE` region per worker
//! up to a per-thread cap, then touch every 4 KiB page in randomized order
//! and rewrite a sentinel. Pairs with the page-file telemetry to confirm we're
//! actually pushing the working set out.
//!
//! On other targets: a fallback that just allocates a `Vec<u8>` and touches
//! pages. Linux can grow a real `mmap`/`madvise` implementation later.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const PAGE_BYTES: usize = 4096;
const CHUNK_MB: u64 = 64;
const TICK: Duration = Duration::from_millis(500);

pub(crate) fn run(
    thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let cap_per_thread_mb = (memory_cap_mb / thread_count.max(1) as u64).max(CHUNK_MB);
    let bytes_counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = bytes_counter.clone();
            thread::Builder::new()
                .name("stress-kit-vm".into())
                .spawn(move || vm_worker(cancel, counter, cap_per_thread_mb))
                .expect("stress-kit: failed to spawn vm worker")
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut last_bytes: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let now_bytes = bytes_counter.load(Ordering::Relaxed);
            let delta = now_bytes.saturating_sub(last_bytes);
            let delta_secs = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mib_per_sec = (delta as f64) / (1024.0 * 1024.0) / delta_secs;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error: None,
                fatal: false,
                errors: 0,
            });

            last_bytes = now_bytes;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

#[cfg(target_os = "windows")]
fn vm_worker(cancel: Arc<AtomicBool>, bytes_counter: Arc<AtomicU64>, cap_mb: u64) {
    use windows::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    };

    let size = (cap_mb as usize).saturating_mul(1024 * 1024).max(PAGE_BYTES);
    let pages = size / PAGE_BYTES;

    let base = unsafe {
        VirtualAlloc(
            None,
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if base.is_null() {
        log::warn!("stress-kit/vm: VirtualAlloc({size}) failed");
        return;
    }

    let guard = VirtualGuard { base, size };

    // Touch a randomized stride large enough to defeat hardware prefetch.
    let stride = 4099usize;
    let mut idx: usize = 0;
    let mut tag: u8 = 0;

    while !cancel.load(Ordering::Relaxed) {
        let page = idx % pages;
        let offset = page * PAGE_BYTES;
        unsafe {
            let p = (guard.base as *mut u8).add(offset);
            std::ptr::write_volatile(p, tag);
            // Also touch a byte mid-page so the dirty bit is set on a non-header byte.
            std::ptr::write_volatile(p.add(PAGE_BYTES / 2), tag.wrapping_add(0x5A));
        }
        bytes_counter.fetch_add(PAGE_BYTES as u64, Ordering::Relaxed);
        idx = idx.wrapping_add(stride);
        tag = tag.wrapping_add(1);
    }

    drop(guard);
}

#[cfg(target_os = "windows")]
struct VirtualGuard {
    base: *mut core::ffi::c_void,
    size: usize,
}

#[cfg(target_os = "windows")]
unsafe impl Send for VirtualGuard {}

#[cfg(target_os = "windows")]
impl Drop for VirtualGuard {
    fn drop(&mut self) {
        use windows::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
        if !self.base.is_null() {
            unsafe {
                // For MEM_RELEASE, size must be 0.
                let _ = VirtualFree(self.base, 0, MEM_RELEASE);
            }
        }
        let _ = self.size;
    }
}

#[cfg(not(target_os = "windows"))]
fn vm_worker(cancel: Arc<AtomicBool>, bytes_counter: Arc<AtomicU64>, cap_mb: u64) {
    let size = (cap_mb as usize).saturating_mul(1024 * 1024).max(PAGE_BYTES);
    let mut buf = vec![0u8; size];
    let pages = size / PAGE_BYTES;
    let stride = 4099usize;
    let mut idx: usize = 0;
    let mut tag: u8 = 0;
    while !cancel.load(Ordering::Relaxed) {
        let page = idx % pages;
        let offset = page * PAGE_BYTES;
        unsafe {
            std::ptr::write_volatile(buf.as_mut_ptr().add(offset), tag);
            std::ptr::write_volatile(buf.as_mut_ptr().add(offset + PAGE_BYTES / 2), tag.wrapping_add(0x5A));
        }
        bytes_counter.fetch_add(PAGE_BYTES as u64, Ordering::Relaxed);
        idx = idx.wrapping_add(stride);
        tag = tag.wrapping_add(1);
    }
    drop(buf);
}

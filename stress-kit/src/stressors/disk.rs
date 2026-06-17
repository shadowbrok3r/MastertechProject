//! Write/read/delete cycles on `std::env::temp_dir()` files; `disk_file_mb` per cycle per worker.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::Metrics;

const TICK: Duration = Duration::from_millis(500);
// Per-worker I/O buffer size; the file is read/written in chunks of this.
const IO_CHUNK_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn run(
    thread_count: usize,
    disk_file_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let bytes_counter = Arc::new(AtomicU64::new(0));
    let error_slot: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

    let handles: Vec<_> = (0..thread_count)
        .enumerate()
        .map(|(id, _)| {
            let cancel = cancel.clone();
            let bytes_counter = bytes_counter.clone();
            let error_slot = error_slot.clone();
            thread::Builder::new()
                .name(format!("stress-kit-disk-{id}"))
                .spawn(move || disk_worker(id, disk_file_mb, cancel, bytes_counter, error_slot))
                .expect("stress-kit: failed to spawn disk worker")
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

            let last_error = error_slot.lock().ok().and_then(|g| g.clone());

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error,
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

fn disk_worker(
    id: usize,
    file_mb: u64,
    cancel: Arc<AtomicBool>,
    bytes_counter: Arc<AtomicU64>,
    error_slot: Arc<std::sync::Mutex<Option<String>>>,
) {
    let file_bytes = (file_mb as usize).max(1) * 1024 * 1024;
    let chunk_bytes = IO_CHUNK_BYTES.min(file_bytes);
    let mut write_buf = vec![0u8; chunk_bytes];
    for (i, b) in write_buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8 ^ 0x5A;
    }
    let mut read_buf = vec![0u8; chunk_bytes];

    let path = std::env::temp_dir().join(format!("stress-kit-{id}-{}.tmp", std::process::id()));

    while !cancel.load(Ordering::Relaxed) {
        if let Err(e) =
            write_and_read(&path, file_bytes, &write_buf, &mut read_buf, &cancel, &bytes_counter)
        {
            if let Ok(mut slot) = error_slot.lock() {
                *slot = Some(format!("disk thread {id}: {e}"));
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    let _ = std::fs::remove_file(&path);
}

fn write_and_read(
    path: &std::path::Path,
    file_bytes: usize,
    write_buf: &[u8],
    read_buf: &mut [u8],
    cancel: &AtomicBool,
    bytes_counter: &AtomicU64,
) -> io::Result<()> {
    {
        let mut f = std::fs::File::create(path)?;
        let mut remaining = file_bytes;
        while remaining > 0 {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let n = remaining.min(write_buf.len());
            f.write_all(&write_buf[..n])?;
            bytes_counter.fetch_add(n as u64, Ordering::Relaxed);
            remaining -= n;
        }
        f.sync_data()?;
    }

    {
        let mut f = std::fs::File::open(path)?;
        let mut remaining = file_bytes;
        while remaining > 0 {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let n = remaining.min(read_buf.len());
            f.read_exact(&mut read_buf[..n])?;
            bytes_counter.fetch_add(n as u64, Ordering::Relaxed);
            remaining -= n;
        }
    }

    std::fs::remove_file(path)?;
    Ok(())
}

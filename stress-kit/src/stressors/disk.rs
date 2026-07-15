//! Write/read/delete cycles on `std::env::temp_dir()` files; `disk_file_mb` per cycle per worker.
//! Read-back is byte-compared against the written pattern; mismatches accumulate
//! in `Metrics::errors` with detail in `last_error`.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
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
    let error_counter = Arc::new(AtomicU64::new(0));
    let error_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let handles: Vec<_> = (0..thread_count)
        .enumerate()
        .map(|(id, _)| {
            let cancel = cancel.clone();
            let bytes_counter = bytes_counter.clone();
            let error_counter = error_counter.clone();
            let error_slot = error_slot.clone();
            thread::Builder::new()
                .name(format!("stress-kit-disk-{id}"))
                .spawn(move || {
                    disk_worker(id, disk_file_mb, cancel, bytes_counter, error_counter, error_slot)
                })
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
            let errors = error_counter.load(Ordering::Relaxed);

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: mib_per_sec,
                last_error,
                fatal: false,
                errors,
            });

            last_bytes = now_bytes;
            last_tick = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }
}

struct ErrorSink {
    worker_id: usize,
    counter: Arc<AtomicU64>,
    slot: Arc<Mutex<Option<String>>>,
}

impl ErrorSink {
    fn record(&self, offset: u64, expected: u8, got: u8, extra: u64) {
        self.counter.fetch_add(1 + extra, Ordering::Relaxed);
        let msg = format!(
            "disk[{}] readback mismatch at offset 0x{:X}: expected 0x{:02X} got 0x{:02X}{}",
            self.worker_id,
            offset,
            expected,
            got,
            if extra > 0 {
                format!(" (+{extra} more in chunk)")
            } else {
                String::new()
            }
        );
        log::error!("[stress-kit/disk] {msg}");
        if let Ok(mut g) = self.slot.lock() {
            *g = Some(msg);
        }
    }
}

fn disk_worker(
    id: usize,
    file_mb: u64,
    cancel: Arc<AtomicBool>,
    bytes_counter: Arc<AtomicU64>,
    error_counter: Arc<AtomicU64>,
    error_slot: Arc<Mutex<Option<String>>>,
) {
    let sink = ErrorSink {
        worker_id: id,
        counter: error_counter,
        slot: error_slot.clone(),
    };

    let file_bytes = (file_mb as usize).max(1) * 1024 * 1024;
    let chunk_bytes = IO_CHUNK_BYTES.min(file_bytes);
    let mut write_buf = vec![0u8; chunk_bytes];
    for (i, b) in write_buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8 ^ 0x5A;
    }
    let mut read_buf = vec![0u8; chunk_bytes];

    let path = std::env::temp_dir().join(format!("stress-kit-{id}-{}.tmp", std::process::id()));

    while !cancel.load(Ordering::Relaxed) {
        if let Err(e) = write_and_read(
            &path,
            file_bytes,
            &write_buf,
            &mut read_buf,
            &cancel,
            &bytes_counter,
            &sink,
        ) {
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
    sink: &ErrorSink,
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
            let offset = (file_bytes - remaining) as u64;
            let n = remaining.min(read_buf.len());
            f.read_exact(&mut read_buf[..n])?;
            bytes_counter.fetch_add(n as u64, Ordering::Relaxed);
            verify_chunk(offset, &write_buf[..n], &read_buf[..n], sink);
            remaining -= n;
        }
    }

    std::fs::remove_file(path)?;
    Ok(())
}

/// Byte-compares `got` against `expected`; on mismatch counts every differing
/// byte and records the first at `offset + index`.
fn verify_chunk(offset: u64, expected: &[u8], got: &[u8], sink: &ErrorSink) {
    if got == expected {
        return;
    }
    let mut mismatches: u64 = 0;
    let mut first: Option<(usize, u8, u8)> = None;
    for (i, (&g, &e)) in got.iter().zip(expected).enumerate() {
        if g != e {
            mismatches += 1;
            if first.is_none() {
                first = Some((i, e, g));
            }
        }
    }
    if let Some((i, e, g)) = first {
        sink.record(offset + i as u64, e, g, mismatches - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> (ErrorSink, Arc<AtomicU64>, Arc<Mutex<Option<String>>>) {
        let counter = Arc::new(AtomicU64::new(0));
        let slot = Arc::new(Mutex::new(None));
        (
            ErrorSink {
                worker_id: 0,
                counter: counter.clone(),
                slot: slot.clone(),
            },
            counter,
            slot,
        )
    }

    #[test]
    fn verify_catches_corrupted_bytes() {
        let (sink, counter, slot) = sink();
        let expected: Vec<u8> = (0..4096u32).map(|i| (i & 0xFF) as u8 ^ 0x5A).collect();
        let mut got = expected.clone();
        got[100] ^= 0xFF;
        got[200] ^= 0x01;

        verify_chunk(0x1_0000, &expected, &got, &sink);

        assert_eq!(counter.load(Ordering::Relaxed), 2);
        let msg = slot.lock().unwrap().clone().expect("error detail recorded");
        assert!(msg.contains("offset 0x10064"), "msg: {msg}");
        assert!(msg.contains("(+1 more in chunk)"), "msg: {msg}");
    }

    #[test]
    fn verify_clean_chunk_counts_nothing() {
        let (sink, counter, _slot) = sink();
        let expected: Vec<u8> = (0..4096u32).map(|i| (i & 0xFF) as u8 ^ 0x5A).collect();
        verify_chunk(0, &expected, &expected.clone(), &sink);
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }
}

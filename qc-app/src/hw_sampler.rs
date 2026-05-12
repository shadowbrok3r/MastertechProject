//! Polls logical CPUs on a thread: usage %, MHz, optional °C via `sysinfo::Components`.
//! Windows CPU temp often unavailable unless the OS exposes sensors `sysinfo` can read.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sysinfo::{CpuRefreshKind, RefreshKind, System};

/// One logical core row for [`crate::hw_table`].
#[derive(Debug, Clone, Default)]
pub struct CoreRow {
    pub index: usize,
    /// CPU marketing name.
    pub brand: String,
    /// Logical core id from `sysinfo` (e.g. `cpu3`).
    pub name: String,
    /// 0–100.
    pub usage_pct: f32,
    /// MHz.
    pub freq_mhz: u64,
    /// °C when a matching `Component` exists.
    pub temp_c: Option<f32>,
}

/// Background thread + latest row snapshot.
pub struct HwSampler {
    rows: Arc<Mutex<Vec<CoreRow>>>,
    cancel: Arc<AtomicBool>,
}

impl HwSampler {
    /// Poll interval; clamped ≥ 100 ms.
    pub fn start(refresh_ms: u64) -> Self {
        let rows: Arc<Mutex<Vec<CoreRow>>> = Arc::new(Mutex::new(Vec::new()));
        let cancel = Arc::new(AtomicBool::new(false));

        let rows_clone = rows.clone();
        let cancel_clone = cancel.clone();
        let interval = Duration::from_millis(refresh_ms.max(100));

        thread::Builder::new()
            .name("qc-hw-sampler".into())
            .spawn(move || sampler_loop(rows_clone, cancel_clone, interval))
            .expect("hw-sampler: failed to spawn thread");

        Self { rows, cancel }
    }

    /// Clone of last sample; empty until first tick completes.
    pub fn get(&self) -> Vec<CoreRow> {
        self.rows.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Signal the background thread to stop.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for HwSampler {
    fn drop(&mut self) {
        self.stop();
    }
}

fn sampler_loop(
    rows: Arc<Mutex<Vec<CoreRow>>>,
    cancel: Arc<AtomicBool>,
    interval: Duration,
) {
    let refresh_kind = RefreshKind::new().with_cpu(CpuRefreshKind::everything());
    let mut sys = System::new_with_specifics(refresh_kind);

    // First `refresh_cpu_all` seeds counters; second pass (after sleep) yields usable %.
    sys.refresh_cpu_all();
    thread::sleep(interval);

    let mut components = sysinfo::Components::new_with_refreshed_list();

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        sys.refresh_cpu_all();

        components.refresh();

        let new_rows: Vec<CoreRow> = sys
            .cpus()
            .iter()
            .enumerate()
            .map(|(i, cpu)| {
                let temp_c = read_core_temp(&components, i);
                CoreRow {
                    index: i,
                    brand: cpu.brand().to_string(),
                    name: cpu.name().to_string(),
                    usage_pct: cpu.cpu_usage(),
                    freq_mhz: cpu.frequency(),
                    temp_c,
                }
            })
            .collect();

        if let Ok(mut g) = rows.lock() {
            *g = new_rows;
        }

        thread::sleep(interval);
    }

    log::debug!("hw-sampler: thread exiting");
}

/// Map `Components` label to a temperature for core `idx` (`Core N` first, else CPU/package).
fn read_core_temp(components: &sysinfo::Components, idx: usize) -> Option<f32> {

    let core_label = format!("Core {idx}");
    if let Some(c) = components.iter().find(|c| c.label() == core_label) {
        return Some(c.temperature());
    }

    components
        .iter()
        .find(|c| {
            let l = c.label().to_lowercase();
            l.contains("package") || l.contains("physical id 0") || l.contains("cpu")
        })
        .map(|c| c.temperature())
}

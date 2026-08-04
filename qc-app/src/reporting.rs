//! Background task: bounded queue of reports/heartbeats, POST with exponential backoff.
//! Empty base URL = no HTTP. UI only `try_send`s; never blocks on the response.

use std::sync::Arc;
use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use sysinfo::{CpuRefreshKind, RefreshKind, System};

use crate::telemetry::{Heartbeat, QcReport};

/// First retry delay (seconds) after a failed POST; doubles until [`MAX_BACKOFF_SECS`].
const INITIAL_BACKOFF_SECS: u64 = 2;
/// Max retry delay (seconds).
const MAX_BACKOFF_SECS: u64 = 120;
/// Queue cap; oldest item dropped when full.
const QUEUE_CAPACITY: usize = 256;

/// Handle to the sink; clone shares one channel + task.
#[derive(Clone)]
pub struct ReportSink {
    tx: Sender<SinkItem>,
    pub machine_id: Arc<String>,
}

impl ReportSink {
    /// `orchestrator_base_url` empty or `None` = log-only. Example: `http://192.168.1.50:7700`.
    pub fn start(orchestrator_base_url: Option<String>, machine_id: String) -> Self {
        let (tx, rx) = crossbeam::channel::bounded::<SinkItem>(QUEUE_CAPACITY);
        let base_url = orchestrator_base_url.unwrap_or_default();
        let machine_id = Arc::new(machine_id);

        tokio::spawn(sink_task(rx, base_url.clone(), machine_id.clone()));
        tokio::spawn(pending_upload_task(base_url, machine_id.clone()));

        Self { tx, machine_id }
    }

    /// `try_send`; drops if queue full.
    pub fn send_report(&self, report: QcReport) {
        let _ = self.tx.try_send(SinkItem::Report(report));
    }

    /// `try_send`; drops if queue full.
    pub fn send_heartbeat(&self, hb: Heartbeat) {
        let _ = self.tx.try_send(SinkItem::Heartbeat(hb));
    }
}

enum SinkItem {
    Report(QcReport),
    Heartbeat(Heartbeat),
}

impl SinkItem {
    fn endpoint(&self) -> &'static str {
        match self {
            SinkItem::Report(_)    => "/api/v1/qc/report",
            SinkItem::Heartbeat(_) => "/api/v1/qc/heartbeat",
        }
    }

    fn to_json_bytes(&self) -> anyhow::Result<Vec<u8>> {
        match self {
            SinkItem::Report(r)    => Ok(serde_json::to_vec(r)?),
            SinkItem::Heartbeat(h) => Ok(serde_json::to_vec(h)?),
        }
    }
}

async fn sink_task(rx: Receiver<SinkItem>, base_url: String, machine_id: Arc<String>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client build failed");

    let dry_run = base_url.is_empty();
    if dry_run {
        log::info!("[reporting] no orchestrator URL configured — running in dry-run mode");
    } else {
        log::info!("[reporting] sink started → {base_url}");
    }

    loop {
        // `recv` blocks; run off the async runtime worker.
        let item = match tokio::task::spawn_blocking({
            let rx = rx.clone();
            move || rx.recv()
        }).await {
            Ok(Ok(item)) => item,
            _ => break, // `ReportSink` dropped
        };

        if dry_run {
            match &item {
                SinkItem::Report(r)    => log::debug!("[reporting/dry-run] report for {}", r.machine_id),
                SinkItem::Heartbeat(h) => log::debug!("[reporting/dry-run] heartbeat for {}", h.machine_id),
            }
            continue;
        }

        let endpoint = item.endpoint();
        let url = format!("{base_url}{endpoint}");
        let bytes = match item.to_json_bytes() {
            Ok(b) => b,
            Err(e) => { log::error!("[reporting] serialize error: {e}"); continue; }
        };

        let mut backoff = INITIAL_BACKOFF_SECS;
        loop {
            match client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("X-Machine-Id", machine_id.as_str())
                .body(bytes.clone())
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    log::debug!("[reporting] {endpoint} → {}", resp.status());
                    break;
                }
                Ok(resp) => {
                    log::warn!("[reporting] {endpoint} → HTTP {} — retrying in {backoff}s", resp.status());
                }
                Err(e) => {
                    log::warn!("[reporting] {endpoint} → {e} — retrying in {backoff}s");
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
        }
    }

    log::info!("[reporting] sink task exiting");
}

/// Drains disk-queued offline results to the orchestrator on startup and every 5 minutes.
async fn pending_upload_task(base_url: String, machine_id: Arc<String>) {
    if base_url.is_empty() {
        return;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client build failed");
    let url = format!("{base_url}/api/v1/qc/report");
    loop {
        let queued = tokio::task::spawn_blocking(crate::pending_results::load_all)
            .await
            .unwrap_or_default();
        for (path, value) in queued {
            match client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("X-Machine-Id", machine_id.as_str())
                .json(&value)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let _ = crate::pending_results::delete(&path);
                    log::info!("[reporting] uploaded queued result {}", path.display());
                }
                Ok(resp) => log::warn!("[reporting] queued upload {} → HTTP {}", path.display(), resp.status()),
                Err(e) => log::debug!("[reporting] queued upload {} failed: {e}", path.display()),
            }
        }
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}

// Machine id: same string as Mastertech `generate_client_id` (SHA-256 hex).

/// `hostname-cpu-PROCESSOR_IDENTIFIER` → SHA-256 hex (`PROCESSOR_IDENTIFIER` missing → `"unknown-cpu"`).
///
/// The inputs (and therefore the output) don't change at runtime, so the
/// two `log::debug!` lines here are intentionally not `info!`: the function
/// can be called from per-frame UI code, and an `info!` would flood the
/// terminal whenever the user moves their mouse.
pub fn generate_client_id(hostname: String, cpu: String) -> String {
    stress_runner::generate_client_hash(&hostname, &cpu)
}

pub fn host_name_and_cpu_brand() -> (String, String) {
    // The offline install's hostname under WinPE, so the record-key prefix stays
    // consistent with the hash `generate_client_hash` derives.
    let hostname = Some(stress_runner::identity_hostname())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string());
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
    );
    sys.refresh_cpu_list(CpuRefreshKind::everything());
    let cpu = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-cpu-brand".to_string());
    (hostname, cpu)
}

/// Deterministic id; writes `machine_id.txt` under MastertechQC data when it differs from disk.
pub fn machine_id() -> String {
    let path = directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("machine_id.txt"));

    let (hostname, cpu) = host_name_and_cpu_brand();
    let id = generate_client_id(hostname, cpu);

    if let Some(p) = path {
        let should_write = match std::fs::read_to_string(&p) {
            Ok(existing) => existing.trim() != id.as_str(),
            Err(_) => true,
        };
        if should_write {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&p, &id);
        }
    }

    id
}
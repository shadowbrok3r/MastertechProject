//! Admin-side bookkeeping + dispatch for remote `plugin_builder`
//! workers.
//!
//! Topology recap: workers connect to `websocket_server2` as
//! `role=client` in a room named `build_worker_<host>`. An admin
//! Mastertech4.0 instance opens a `WebSocketClient` session to each
//! such room exactly like a customer-machine session, and binary
//! frames flow through the existing `RemoteEguiControlHub`. Builder
//! traffic is multiplexed on that channel using
//! [`plugin_builder::BUILDER_WIRE_TAG`] — the receive handler in
//! `client_interface/receive.rs` detects the tag and routes here.
//!
//! This module owns three things:
//! 1. The registry of online workers and what each advertised in its
//!    `Hello` (so `list_build_workers` is just a snapshot read).
//! 2. The pending-jobs table (`BUILD_JOB_PENDING`), keyed by
//!    `job_id`. State machine: `Pending → Done(bytes)` or `Failed`.
//! 3. Outbound dispatch — `send_compile_request` constructs a
//!    `BuilderWire::CompileRequest`, tags it, and ships it via the
//!    hub.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use plugin_builder::{BuilderWire, CompileProfile, CompileTarget};

/// How long since a worker's last `Hello` before we treat it as
/// gone. Worker heartbeat is every 30s (`HEARTBEAT_INTERVAL` in the
/// worker); three missed heartbeats are enough to declare it dead.
const STALE_AFTER: Duration = Duration::from_secs(90);

/// Snapshot of one worker's `Hello`, kept so MCP can show it in
/// `list_build_workers` without waiting on the worker to round-trip.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    pub connection_string: String,
    pub hostname: String,
    pub target_triples: Vec<String>,
    pub capabilities: Vec<String>,
    pub worker_version: String,
    pub registered_at: Instant,
}

/// Terminal state for a compile job. `Pending` means the worker is
/// still chewing on it (or we never heard back); `Done` carries the
/// `.wasm` bytes ready to drop into `ArtifactStore`; `Failed` carries
/// the stderr the agent needs to read.
#[derive(Debug, Clone)]
pub enum BuildJobState {
    Pending {
        worker: String,
        plugin_id: String,
        started_at: Instant,
    },
    Done {
        worker: String,
        plugin_id: String,
        wasm_bytes: Vec<u8>,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    },
    Failed {
        worker: String,
        plugin_id: String,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    },
}

impl BuildJobState {
    pub fn status_str(&self) -> &'static str {
        match self {
            BuildJobState::Pending { .. } => "pending",
            BuildJobState::Done { .. } => "done",
            BuildJobState::Failed { .. } => "failed",
        }
    }
}

static WORKER_REGISTRY: Lazy<Mutex<HashMap<String, WorkerInfo>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static BUILD_JOB_PENDING: Lazy<Mutex<HashMap<String, BuildJobState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── Worker registry ────────────────────────────────────────────────

/// Called by the receive handler when a worker's `Hello` arrives.
/// Idempotent — re-sending `Hello` just refreshes the entry.
pub fn register_worker(connection_string: &str, hello: BuilderWire) {
    let BuilderWire::Hello {
        hostname,
        target_triples,
        capabilities,
        worker_version,
    } = hello
    else {
        log::warn!(
            "register_worker called with non-Hello variant for {}",
            connection_string
        );
        return;
    };
    let info = WorkerInfo {
        connection_string: connection_string.to_string(),
        hostname,
        target_triples,
        capabilities,
        worker_version,
        registered_at: Instant::now(),
    };
    log::info!(
        "build worker registered: {} ({}) targets={:?}",
        info.hostname,
        info.connection_string,
        info.target_triples
    );
    if let Ok(mut reg) = WORKER_REGISTRY.lock() {
        reg.insert(connection_string.to_string(), info);
    }
}

/// Drop a worker on disconnect. Pending jobs assigned to it stay in
/// the table so `plugin_compile_status` can surface "worker
/// disconnected"; callers can retry.
pub fn unregister_worker(connection_string: &str) {
    if let Ok(mut reg) = WORKER_REGISTRY.lock() {
        if reg.remove(connection_string).is_some() {
            log::info!("build worker unregistered: {}", connection_string);
        }
    }
}

/// List workers whose last `Hello` was within [`STALE_AFTER`].
/// Stale entries are pruned in-place each call so a worker that
/// drops without sending Close eventually disappears on its own.
pub fn list_workers() -> Vec<WorkerInfo> {
    let mut reg = match WORKER_REGISTRY.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let stale: Vec<String> = reg
        .iter()
        .filter(|(_, w)| w.registered_at.elapsed() > STALE_AFTER)
        .map(|(k, _)| k.clone())
        .collect();
    for k in &stale {
        log::debug!("pruning stale build worker {}", k);
        reg.remove(k);
    }
    reg.values().cloned().collect()
}

/// Pick an arbitrary fresh (non-stale) worker that advertises
/// `target`. Used as the default routing when `plugin_compile_remote`
/// is called without `worker_connection_string`.
pub fn pick_worker_for(target: &str) -> Option<WorkerInfo> {
    // Reuse the prune pass so callers don't dispatch to a dead worker.
    list_workers()
        .into_iter()
        .find(|w| w.target_triples.iter().any(|t| t == target))
}

// ── Pending jobs ───────────────────────────────────────────────────

/// Register a job before sending it — keeps `plugin_compile_status`
/// honest if the very-first poll arrives before the worker replies.
pub fn register_pending(job_id: &str, worker: &str, plugin_id: &str) {
    if let Ok(mut map) = BUILD_JOB_PENDING.lock() {
        map.insert(
            job_id.to_string(),
            BuildJobState::Pending {
                worker: worker.to_string(),
                plugin_id: plugin_id.to_string(),
                started_at: Instant::now(),
            },
        );
    }
}

/// Called from the receive path when a `BuilderWire::CompileResult`
/// arrives. Overwrites the job's `Pending` entry with the terminal
/// state.
pub fn resolve_job(job_id: &str, result: BuilderWire) {
    let BuilderWire::CompileResult {
        job_id: _,
        success,
        wasm_bytes,
        stdout,
        stderr,
        duration_ms,
    } = result
    else {
        log::warn!("resolve_job called with non-CompileResult variant");
        return;
    };
    let Ok(mut map) = BUILD_JOB_PENDING.lock() else {
        log::error!("BUILD_JOB_PENDING poisoned");
        return;
    };
    let Some(prev) = map.get(job_id).cloned() else {
        log::warn!("resolve_job for unknown job_id {}", job_id);
        return;
    };
    let (worker, plugin_id) = match prev {
        BuildJobState::Pending {
            worker, plugin_id, ..
        } => (worker, plugin_id),
        BuildJobState::Done {
            worker, plugin_id, ..
        }
        | BuildJobState::Failed {
            worker, plugin_id, ..
        } => (worker, plugin_id),
    };
    let new_state = if success {
        BuildJobState::Done {
            worker,
            plugin_id,
            wasm_bytes: wasm_bytes.unwrap_or_default(),
            stdout,
            stderr,
            duration_ms,
        }
    } else {
        BuildJobState::Failed {
            worker,
            plugin_id,
            stdout,
            stderr,
            duration_ms,
        }
    };
    log::info!("[{job_id}] resolved → {}", new_state.status_str());
    map.insert(job_id.to_string(), new_state);
}

/// Surface progress in logs. We don't store these — they're a UX hint,
/// not part of the job state machine.
pub fn record_progress(job_id: &str, progress: BuilderWire) {
    if let BuilderWire::CompileProgress {
        stage, message, ..
    } = progress
    {
        log::info!("[{job_id}] {stage}: {message}");
    }
}

/// Read-only snapshot of one job's current state. The MCP polling
/// tool calls this each tick.
pub fn job_state(job_id: &str) -> Option<BuildJobState> {
    BUILD_JOB_PENDING
        .lock()
        .ok()
        .and_then(|m| m.get(job_id).cloned())
}

/// Remove a finished job after the MCP tool has consumed the bytes
/// (or the user wants to forget a failure). Optional cleanup.
pub fn forget_job(job_id: &str) {
    if let Ok(mut m) = BUILD_JOB_PENDING.lock() {
        m.remove(job_id);
    }
}

// ── Outbound dispatch ──────────────────────────────────────────────

/// Build a tagged `BuilderWire::CompileRequest` frame and ship it via
/// the admin's existing remote-egui hub (same path
/// `plugin_deploy_remote` uses). Caller owns the `job_id` and is
/// expected to have already called [`register_pending`] for it.
pub fn send_compile_request(
    worker_connection_string: &str,
    job_id: &str,
    plugin_id: &str,
    cargo_toml: &str,
    lib_rs: &str,
    target: &CompileTarget,
    profile: &CompileProfile,
) -> Result<(), String> {
    let frame = BuilderWire::CompileRequest {
        job_id: job_id.to_string(),
        plugin_id: plugin_id.to_string(),
        cargo_toml: cargo_toml.to_string(),
        lib_rs: lib_rs.to_string(),
        target: target.clone(),
        profile: profile.clone(),
    };
    let bytes = frame
        .encode_tagged()
        .map_err(|e| format!("bincode encode: {e}"))?;
    super::remote_egui_control::hub()
        .send_raw_binary(worker_connection_string, bytes)
}

//! Auto-ingest dump-decode analysis results into fleet crash-signature intelligence.
//!
//! `WebSocketClient::receive` routes every `Cmd::RemotePluginToolResult` from
//! `com.mastertech.dump-decode` analysis tools here. Each decoded dump is
//! normalized into a `crash_signature` upsert + `crash_sighting`; prior verdicts
//! surface as toasts, per-client notices, and diagnostic-session findings.

use std::collections::HashMap;
use std::sync::Mutex;

use database::schema::{
    crash_intel::{
        parse_dump_decode_payload, parse_gpu_crash_payload, parse_kernel_triage_payload,
        payload_status, CrashIngest, CrashSignature, ParsedCrash, SightingContext, GPU_DUMP_KIND,
    },
    DiagnosticCategory, DiagnosticEntry, DiagnosticSession, PluginUsageRef, RecordId,
    DIAGNOSTIC_SESSION_TABLE,
};
use once_cell::sync::Lazy;

use crate::{get_toast_sender, PlatformSpawner, Spawner, ToastMessage};

pub const DUMP_DECODE_PLUGIN_ID: &str = "com.mastertech.dump-decode";
const ANALYZE_TOOLS: [&str; 3] = ["read_batch", "read_analyze", "read_analyze_livekernel"];

/// Native (non-plugin) remote crash-dump analysis, reported via a reused
/// `RemotePluginToolResult` with this pseudo plugin id.
pub const NATIVE_CRASH_ANALYSIS_ID: &str = "native.crash-analysis";
/// In-wasm PAGEDU64 triage plugin.
pub const DUMP_TRIAGE_PLUGIN_ID: &str = "com.mastertech.dump-triage";
const TRIAGE_TOOLS: [&str; 3] = ["analyze_crash_dumps", "triage_dump", "triage_all"];

/// Latest ingest batch per connection_string, read by the Crash Intel viewer.
static LATEST_INGESTS: Lazy<Mutex<HashMap<String, Vec<CrashIngest>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Pending per-client notice lines drained into the session history each frame.
static NOTICES: Lazy<Mutex<HashMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// True for dump-decode (cdb `!analyze`) results worth ingesting.
pub fn is_dump_analysis_result(plugin_id: &str, tool_name: &str) -> bool {
    plugin_id == DUMP_DECODE_PLUGIN_ID && ANALYZE_TOOLS.contains(&tool_name)
}

/// True for native remote analysis / dump-triage plugin results (PAGEDU64
/// triage JSON) worth ingesting.
pub fn is_kernel_triage_result(plugin_id: &str, tool_name: &str) -> bool {
    (plugin_id == NATIVE_CRASH_ANALYSIS_ID || plugin_id == DUMP_TRIAGE_PLUGIN_ID)
        && TRIAGE_TOOLS.contains(&tool_name)
}

fn push_notice(connection_string: &str, notice: String) {
    if let Ok(mut map) = NOTICES.lock() {
        map.entry(connection_string.to_string())
            .or_default()
            .push(notice);
    }
}

/// Notice queue shared with the other intel hooks (drained into session history).
pub fn push_shared_notice(connection_string: &str, notice: String) {
    push_notice(connection_string, notice);
}

/// Take any pending notices for a client session.
pub fn drain_notices(connection_string: &str) -> Vec<String> {
    NOTICES
        .lock()
        .ok()
        .and_then(|mut map| map.remove(connection_string))
        .unwrap_or_default()
}

/// Latest ingest batch for a client session.
pub fn latest_ingests(connection_string: &str) -> Vec<CrashIngest> {
    LATEST_INGESTS
        .lock()
        .ok()
        .and_then(|map| map.get(connection_string).cloned())
        .unwrap_or_default()
}

fn verdict_summary(ingest: &CrashIngest) -> Option<String> {
    let v = ingest.verdicts.first()?;
    let fix = if v.fix.is_empty() {
        String::new()
    } else {
        format!(" Fix: {}", v.fix)
    };
    Some(format!(
        "[{} {}] KNOWN CRASH ({} prior sighting(s), {} machine(s)) — {}.{}",
        ingest.signature.bugcheck_code,
        ingest.signature.module,
        ingest.prior_sighting_count,
        ingest.prior_machine_count,
        v.verdict,
        fix
    ))
}

async fn log_finding(session_ref: &RecordId, ingest: &CrashIngest, tool_name: &str) {
    let detail = match verdict_summary(ingest) {
        Some(s) => s,
        None => format!(
            "[{} {}] New fleet signature (first sighting){}",
            ingest.signature.bugcheck_code,
            ingest.signature.module,
            ingest
                .signature
                .failure_buckets
                .first()
                .map(|b| format!(" — bucket {b}"))
                .unwrap_or_default()
        ),
    };
    let entry = DiagnosticEntry {
        session_ref: session_ref.clone(),
        category: DiagnosticCategory::Finding,
        title: format!(
            "Crash signature {} {}",
            ingest.signature.bugcheck_code, ingest.signature.module
        ),
        detail,
        data: serde_json::to_value(ingest).ok(),
        plugins_used: vec![PluginUsageRef {
            plugin_id: DUMP_DECODE_PLUGIN_ID.to_string(),
            tool_name: tool_name.to_string(),
        }],
        ..Default::default()
    };
    if let Err(e) = DiagnosticEntry::create(&entry).await {
        log::warn!("crash_intel: failed to log diagnostic entry: {e}");
    }
}

/// Parse a dump-decode (cdb `!analyze`) payload and persist signatures/sightings.
pub fn ingest_dump_decode_result(
    connection_string: String,
    computer: Option<RecordId>,
    tool_name: String,
    result_json: String,
) {
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&result_json) else {
        return;
    };
    if payload_status(&payload).as_deref() != Some("done") {
        return;
    }
    let crashes = parse_dump_decode_payload(&payload);
    if crashes.is_empty() {
        return;
    }
    let dump_kind = if tool_name.contains("livekernel") {
        "livekernel"
    } else {
        "minidump"
    };
    spawn_ingest(connection_string, computer, tool_name, dump_kind, crashes);
}

/// Parse a dump-triage (PAGEDU64) payload — native remote analysis, local
/// `minidump_analyze`, or the `com.mastertech.dump-triage` plugin — and persist
/// signatures/sightings. This is the guaranteed-logging path: every dump the
/// tools decode lands in `crash_signature`/`crash_sighting`.
pub fn ingest_kernel_triage_result(
    connection_string: String,
    computer: Option<RecordId>,
    tool_name: String,
    result_json: String,
) {
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&result_json) else {
        return;
    };
    let crashes = parse_kernel_triage_payload(&payload);
    if crashes.is_empty() {
        return;
    }
    spawn_ingest(connection_string, computer, tool_name, "minidump", crashes);
}

/// GPU/UE crash-artifact collector plugin.
pub const GPU_DUMPS_PLUGIN_ID: &str = "com.mastertech.gpu-dumps";

/// True for GPU crash-context results worth ingesting.
pub fn is_gpu_crash_result(plugin_id: &str, tool_name: &str) -> bool {
    plugin_id == GPU_DUMPS_PLUGIN_ID && tool_name == "read_gpu_dump_context"
}

/// Parses a GPU crash context and persists it with `dump_kind = 'gpu_aftermath'`.
pub fn ingest_gpu_crash_result(
    connection_string: String,
    computer: Option<RecordId>,
    tool_name: String,
    result_json: String,
) {
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&result_json) else {
        return;
    };
    let crashes = parse_gpu_crash_payload(&payload);
    if crashes.is_empty() {
        return;
    }
    spawn_ingest(connection_string, computer, tool_name, GPU_DUMP_KIND, crashes);
}

/// Session/task/computer linkage resolved for new crash sightings.
#[derive(Debug, Default, Clone)]
pub struct SightingLinks {
    pub session_ref: Option<RecordId>,
    pub task_ref: Option<RecordId>,
    pub computer: Option<RecordId>,
}

/// Resolve sighting linkage for a connection at ingest time: the
/// registry-pinned session when present, else the newest open
/// `diagnostic_session` row for the connection (or its computer). The task
/// comes from `session.task_ref`, else the connection's open service order.
pub async fn resolve_sighting_links(
    connection_string: &str,
    computer: Option<RecordId>,
) -> SightingLinks {
    let mut session: Option<DiagnosticSession> = None;
    if let Some(key) = super::diagnostic_session_registry::get(connection_string) {
        session = database::db()
            .select(RecordId::new(DIAGNOSTIC_SESSION_TABLE, key))
            .await
            .ok()
            .flatten();
    }
    if session.is_none() {
        match DiagnosticSession::latest_open_for_connection(connection_string, computer.as_ref())
            .await
        {
            Ok(found) => session = found,
            Err(e) => {
                log::warn!("crash_intel: open-session lookup failed for {connection_string}: {e}")
            }
        }
    }
    let Some(session) = session else {
        return SightingLinks { computer, ..Default::default() };
    };
    let task_ref = match session.task_ref.clone() {
        Some(t) => Some(t),
        None => match session.resolve_open_service_task().await {
            Ok(hit) => hit.map(|(task, _service_order)| task),
            Err(e) => {
                log::warn!("crash_intel: service-task lookup failed for {connection_string}: {e}");
                None
            }
        },
    };
    SightingLinks {
        session_ref: Some(session.id.clone()),
        task_ref,
        computer: computer.or(Some(session.computer_id)),
    }
}

/// Shared: upsert each parsed crash into fleet intel and surface prior verdicts.
/// Per-crash `dump_kind` is refined from the triage blob when present.
fn spawn_ingest(
    connection_string: String,
    computer: Option<RecordId>,
    tool_name: String,
    default_dump_kind: &str,
    crashes: Vec<ParsedCrash>,
) {
    let default_dump_kind = default_dump_kind.to_string();
    PlatformSpawner::spawn(async move {
        let links = resolve_sighting_links(&connection_string, computer).await;

        let mut ingests: Vec<CrashIngest> = Vec::new();
        for parsed in &crashes {
            // Refine the coarse dump_kind from the triage blob when present.
            let dump_kind = parsed
                .triage
                .as_ref()
                .and_then(|t| t.get("dump_type_name").and_then(|v| v.as_str()))
                .map(|dt| if dt.contains("live") { "livekernel" } else { "minidump" })
                .unwrap_or(&default_dump_kind)
                .to_string();
            let ctx = SightingContext {
                connection_string: Some(connection_string.clone()),
                computer: links.computer.clone(),
                session_ref: links.session_ref.clone(),
                task_ref: links.task_ref.clone(),
                dump_kind,
            };
            match CrashSignature::ingest(parsed, &ctx).await {
                Ok(ingest) => {
                    if let Some(summary) = verdict_summary(&ingest) {
                        push_notice(&connection_string, summary.clone());
                        let _ = get_toast_sender().try_send(ToastMessage::Warning(summary));
                    } else if ingest.prior_sighting_count > 0 {
                        push_notice(
                            &connection_string,
                            format!(
                                "[{} {}] Seen {} time(s) across {} machine(s) — no verdict recorded yet",
                                ingest.signature.bugcheck_code,
                                ingest.signature.module,
                                ingest.prior_sighting_count,
                                ingest.prior_machine_count.max(1)
                            ),
                        );
                    }
                    if let Some(session_ref) = links.session_ref.as_ref() {
                        log_finding(session_ref, &ingest, &tool_name).await;
                    }
                    ingests.push(ingest);
                }
                Err(e) => log::warn!(
                    "crash_intel: ingest failed for {} {}: {e}",
                    parsed.bugcheck_code,
                    parsed.module
                ),
            }
        }

        if !ingests.is_empty() {
            let known = ingests.iter().filter(|i| i.previously_seen).count();
            push_notice(
                &connection_string,
                format!(
                    "Crash intel: recorded {} signature(s) from {tool_name} ({known} previously known)",
                    ingests.len()
                ),
            );
            if let Ok(mut map) = LATEST_INGESTS.lock() {
                map.insert(connection_string.clone(), ingests);
            }
        }

        surface_cross_dump_diffs(&connection_string, &crashes);
    });
}

/// Diff a batch of parsed triages against the newest and surface driver-set
/// deltas as a per-client notice.
fn surface_cross_dump_diffs(connection_string: &str, crashes: &[ParsedCrash]) {
    // A GPU blob deserializes into an all-default triage, so exclude it by kind.
    let triages: Vec<dump_triage::KernelDumpTriage> = crashes
        .iter()
        .filter_map(|c| c.triage.as_ref())
        .filter(|t| {
            t.get("kind").and_then(|v| v.as_str())
                != Some(dump_triage::gpu::GPU_AFTERMATH_DUMP_KIND)
        })
        .filter_map(|t| serde_json::from_value(t.clone()).ok())
        .collect();
    if triages.len() < 2 {
        return;
    }
    let diffs = dump_triage::diff::baseline_diffs(&triages);
    let mut lines: Vec<String> = Vec::new();
    for d in &diffs {
        if d.drivers_added.is_empty() && d.drivers_removed.is_empty() && d.drivers_rebased.is_empty()
        {
            continue;
        }
        let mut parts: Vec<String> = Vec::new();
        if !d.drivers_added.is_empty() {
            parts.push(format!("added {}", d.drivers_added.join(", ")));
        }
        if !d.drivers_removed.is_empty() {
            parts.push(format!("removed {}", d.drivers_removed.join(", ")));
        }
        if !d.drivers_rebased.is_empty() {
            parts.push(format!("rebased {}", d.drivers_rebased.join(", ")));
        }
        lines.push(parts.join("; "));
    }
    if !lines.is_empty() {
        push_notice(
            connection_string,
            format!("Cross-dump vs newest: {}", lines.join(" | ")),
        );
    }
}

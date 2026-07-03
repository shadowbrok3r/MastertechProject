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
        parse_dump_decode_payload, payload_status, CrashIngest, CrashSignature, SightingContext,
    },
    DiagnosticCategory, DiagnosticEntry, PluginUsageRef, RecordId, DIAGNOSTIC_SESSION_TABLE,
};
use once_cell::sync::Lazy;

use crate::{get_toast_sender, PlatformSpawner, Spawner, ToastMessage};

pub const DUMP_DECODE_PLUGIN_ID: &str = "com.mastertech.dump-decode";
const ANALYZE_TOOLS: [&str; 3] = ["read_batch", "read_analyze", "read_analyze_livekernel"];

/// Latest ingest batch per connection_string, read by the Crash Intel viewer.
static LATEST_INGESTS: Lazy<Mutex<HashMap<String, Vec<CrashIngest>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Pending per-client notice lines drained into the session history each frame.
static NOTICES: Lazy<Mutex<HashMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// True for dump-decode results that carry `!analyze` output worth ingesting.
pub fn is_dump_analysis_result(plugin_id: &str, tool_name: &str) -> bool {
    plugin_id == DUMP_DECODE_PLUGIN_ID && ANALYZE_TOOLS.contains(&tool_name)
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

async fn log_finding(session_key: &str, ingest: &CrashIngest, tool_name: &str) {
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
        session_ref: RecordId::new(DIAGNOSTIC_SESSION_TABLE, session_key),
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

/// Parse a dump-decode analysis payload and persist crash signatures/sightings.
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
    }
    .to_string();

    PlatformSpawner::spawn(async move {
        let session_key = super::diagnostic_session_registry::get(&connection_string);
        let ctx = SightingContext {
            connection_string: Some(connection_string.clone()),
            computer,
            session_ref: session_key
                .as_deref()
                .map(|k| RecordId::new(DIAGNOSTIC_SESSION_TABLE, k)),
            task_ref: None,
            dump_kind,
        };

        let mut ingests: Vec<CrashIngest> = Vec::new();
        for parsed in &crashes {
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
                    if let Some(key) = session_key.as_deref() {
                        log_finding(key, &ingest, &tool_name).await;
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
    });
}

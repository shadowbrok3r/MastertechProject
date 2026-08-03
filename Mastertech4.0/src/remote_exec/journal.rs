//! Append-only on-disk journal of RemoteExec activity.
//!
//! The in-memory ring is bounded and dies with the process, so it cannot be the
//! record of what a technician ran on a paying customer's machine. Every
//! submission, signal and exit is appended here as one JSON line. This is a
//! forensic aid, not an authority — anyone with admin on the box can edit it —
//! so the durable record still belongs in SurrealDB.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde_json::json;

use super::job::now_ms;

fn journal_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    PathBuf::from(base).join("Mastertech").join("remote_exec.jsonl")
}

fn append(value: serde_json::Value) {
    let path = journal_path();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("[remote_exec/journal] cannot create {}: {e}", dir.display());
            return;
        }
    }
    let line = match serde_json::to_string(&value) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[remote_exec/journal] serialize failed: {e}");
            return;
        }
    };
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                log::warn!("[remote_exec/journal] write failed: {e}");
            }
        }
        Err(e) => log::warn!("[remote_exec/journal] open {} failed: {e}", path.display()),
    }
}

pub fn record_submitted(
    job_id: &str,
    tech: &str,
    reason: &str,
    risk: &str,
    spec_summary: &str,
    diagnostic_session_id: Option<&str>,
) {
    append(json!({
        "at_ms": now_ms(),
        "event": "submitted",
        "job_id": job_id,
        "tech": tech,
        "reason": reason,
        "risk": risk,
        "spec": spec_summary,
        "diagnostic_session_id": diagnostic_session_id,
    }));
}

pub fn record_denied(job_id: &str, tech: &str, why: &str) {
    append(json!({
        "at_ms": now_ms(),
        "event": "denied",
        "job_id": job_id,
        "tech": tech,
        "reason": why,
    }));
}

pub fn record_signal(job_id: &str, signal: &str) {
    append(json!({
        "at_ms": now_ms(),
        "event": "signal",
        "job_id": job_id,
        "signal": signal,
    }));
}

pub fn record_exit(
    job_id: &str,
    outcome: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout_bytes: u64,
    stderr_bytes: u64,
    truncated_bytes: u64,
) {
    append(json!({
        "at_ms": now_ms(),
        "event": "exit",
        "job_id": job_id,
        "outcome": outcome,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "stdout_bytes": stdout_bytes,
        "stderr_bytes": stderr_bytes,
        "truncated_bytes": truncated_bytes,
    }));
}

/// Screen capture and input injection. `count` is JPEG bytes for a capture and
/// the event count for input.
pub fn record_screen(kind: &str, count: u64) {
    super::note_screen_activity();
    append(json!({
        "at_ms": now_ms(),
        "event": "screen",
        "kind": kind,
        "count": count,
        "tech": super::gate::banner_info().map(|b| b.tech),
    }));
}

pub fn record_gate(event: &str, tech: &str, diagnostic_session_id: &str, ttl_secs: Option<u64>) {
    append(json!({
        "at_ms": now_ms(),
        "event": event,
        "tech": tech,
        "diagnostic_session_id": diagnostic_session_id,
        "ttl_secs": ttl_secs,
    }));
}

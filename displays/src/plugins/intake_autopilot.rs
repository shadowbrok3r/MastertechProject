//! One-click intake triage for a connected client.
//!
//! Fires the collection suite on the client (crash survey, WMI driver
//! inventory, DriverStore snapshot, detached WinDbg batch analysis), logs each
//! result into a diagnostic session, lets the crash/driver intel hooks persist
//! signatures and blocklist hits, then has Claude Code draft a triage verdict
//! from everything gathered. Progress surfaces through the shared notice queue
//! and toasts; findings land as diagnostic entries.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam::channel::Sender;
use database::schema::{
    crash_intel::payload_status,
    driver_intel::{parse_pnputil_enum, KnownBadDriver},
    ConnectedClient, DiagnosticCategory, DiagnosticEntry, DiagnosticSession, RecordIdExt,
    DIAGNOSTIC_SESSION_TABLE,
};
use once_cell::sync::Lazy;

use crate::ai::claude_code::ClaudeCodeSession;
use crate::tabs::ai_playground::{ChatMessage, ChatMessageType};
use crate::{get_toast_sender, Cmd, PlatformSpawner, Spawner, ToastMessage};

use super::crash_intel_hooks::{self, push_shared_notice, DUMP_DECODE_PLUGIN_ID};
use super::driver_intel_hooks::{self, DRIVERSTORE_PLUGIN_ID};

static TRIAGE_RUNNING: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

/// Whether a triage run is currently in flight (one at a time, fleet-wide).
pub fn triage_running() -> bool {
    TRIAGE_RUNNING.load(Ordering::Acquire)
}

/// Call one remote plugin tool over the session channel and await its result.
async fn call_tool(
    cmd_tx: &Sender<Cmd>,
    plugin_id: &str,
    tool_name: &str,
    timeout_secs: u64,
) -> Option<serde_json::Value> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let rx = super::mcp_bridge::register_pending_request(request_id.clone());
    if cmd_tx
        .try_send(Cmd::CallRemotePluginTool {
            request_id: request_id.clone(),
            plugin_id: plugin_id.to_string(),
            tool_name: tool_name.to_string(),
            args_json: "{}".to_string(),
        })
        .is_err()
    {
        super::mcp_bridge::unregister_pending_request(&request_id);
        return None;
    }
    match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok((true, json))) => serde_json::from_str(&json).ok(),
        Ok(Ok((false, err))) => {
            log::warn!("autopilot: {plugin_id}::{tool_name} failed: {err}");
            None
        }
        _ => {
            super::mcp_bridge::unregister_pending_request(&request_id);
            log::warn!("autopilot: {plugin_id}::{tool_name} timed out");
            None
        }
    }
}

async fn log_entry(
    session_key: Option<&str>,
    category: DiagnosticCategory,
    title: &str,
    detail: String,
    data: Option<serde_json::Value>,
) {
    let Some(key) = session_key else { return };
    let entry = DiagnosticEntry {
        session_ref: database::schema::RecordId::new(DIAGNOSTIC_SESSION_TABLE, key),
        category,
        title: title.to_string(),
        detail,
        data,
        ..Default::default()
    };
    if let Err(e) = DiagnosticEntry::create(&entry).await {
        log::warn!("autopilot: diagnostic entry failed: {e}");
    }
}

fn ensure_session(client: &ConnectedClient) -> Option<String> {
    if let Some(existing) = super::diagnostic_session_registry::get(&client.connection_string) {
        return Some(existing);
    }
    None
}

async fn create_session(client: &ConnectedClient) -> Option<String> {
    let (Some(customer), Some(computer)) = (client.customer.clone(), client.computer.clone())
    else {
        return None;
    };
    let hostname = client
        .connection_string
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string();
    let session = DiagnosticSession {
        connection_string: client.connection_string.clone(),
        hostname,
        customer_id: Some(customer),
        computer_id: Some(computer),
        tags: vec!["autopilot".to_string(), "intake".to_string()],
        ..Default::default()
    };
    match DiagnosticSession::create(&session).await {
        Ok(id) => {
            let key = id.key_string();
            super::diagnostic_session_registry::register(&client.connection_string, &key);
            Some(key)
        }
        Err(e) => {
            log::warn!("autopilot: session create failed: {e}");
            None
        }
    }
}

fn summarize_survey(survey: &serde_json::Value) -> String {
    let data = survey.get("data").unwrap_or(survey);
    let g = |k: &str| data.get(k).cloned().unwrap_or(serde_json::Value::Null);
    format!(
        "minidumps={} livekernel={} kernel_power_41={} os={}",
        g("minidump_count"),
        g("livekernel_count"),
        g("kernel_power_41"),
        data.pointer("/os/caption").cloned().unwrap_or_default()
    )
}

/// Run the full intake triage suite against one connected client.
pub fn run_intake_triage(client: ConnectedClient, cmd_tx: Sender<Cmd>, with_ai: bool) {
    if TRIAGE_RUNNING.swap(true, Ordering::AcqRel) {
        let _ = get_toast_sender().try_send(ToastMessage::Warning(
            "Intake triage already running — wait for it to finish.".into(),
        ));
        return;
    }
    let cs = client.connection_string.clone();
    push_shared_notice(&cs, "Intake triage started".to_string());
    let _ = get_toast_sender().try_send(ToastMessage::Info(format!(
        "Intake triage started for {cs}"
    )));

    PlatformSpawner::spawn(async move {
        let result = triage_inner(&client, &cmd_tx, with_ai).await;
        TRIAGE_RUNNING.store(false, Ordering::Release);
        let msg = match result {
            Ok(summary) => {
                push_shared_notice(&client.connection_string, summary.clone());
                format!("Intake triage complete for {}", client.connection_string)
            }
            Err(e) => {
                push_shared_notice(
                    &client.connection_string,
                    format!("Intake triage aborted: {e}"),
                );
                format!("Intake triage aborted: {e}")
            }
        };
        let _ = get_toast_sender().try_send(ToastMessage::Info(msg));
    });
}

async fn triage_inner(
    client: &ConnectedClient,
    cmd_tx: &Sender<Cmd>,
    with_ai: bool,
) -> anyhow::Result<String> {
    let cs = client.connection_string.clone();

    let session_key = match ensure_session(client) {
        Some(k) => Some(k),
        None => {
            let created = create_session(client).await;
            match &created {
                Some(k) => push_shared_notice(&cs, format!("Diagnostic session opened ({k})")),
                None => push_shared_notice(
                    &cs,
                    "No customer/computer link — running triage without a diagnostic session"
                        .to_string(),
                ),
            }
            created
        }
    };
    let sk = session_key.as_deref();

    // 1. Crash survey.
    push_shared_notice(&cs, "Triage 1/4: crash survey".to_string());
    let survey = call_tool(cmd_tx, DUMP_DECODE_PLUGIN_ID, "survey", 90).await;
    let survey_summary = survey.as_ref().map(|s| summarize_survey(s)).unwrap_or_else(|| "unavailable".into());
    if let Some(s) = &survey {
        log_entry(
            sk,
            DiagnosticCategory::SystemInfo,
            "Autopilot: crash survey",
            survey_summary.clone(),
            Some(s.clone()),
        )
        .await;
    }

    // 2. WMI driver inventory (third-party, oldest first).
    push_shared_notice(&cs, "Triage 2/4: driver inventory".to_string());
    let wmi_drivers = call_tool(cmd_tx, DUMP_DECODE_PLUGIN_ID, "drivers", 90).await;
    let mut oldest_drivers = String::new();
    if let Some(d) = &wmi_drivers {
        let rows = database::schema::driver_intel::parse_wmi_driver_payload(d);
        oldest_drivers = rows
            .iter()
            .take(8)
            .map(|r| {
                format!(
                    "{} {} ({})",
                    r.device_name.as_deref().unwrap_or(&r.original_name),
                    r.driver_version,
                    r.driver_date
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        log_entry(
            sk,
            DiagnosticCategory::SystemInfo,
            "Autopilot: WMI driver inventory",
            format!("{} third-party drivers, oldest: {oldest_drivers}", rows.len()),
            None,
        )
        .await;
    }

    // 3. DriverStore snapshot + blocklist match (hook persists the snapshot row).
    push_shared_notice(&cs, "Triage 3/4: DriverStore snapshot".to_string());
    driver_intel_hooks::set_pending_label(&cs, "intake");
    let snapshot = call_tool(cmd_tx, DRIVERSTORE_PLUGIN_ID, "snapshot", 150).await;
    let mut known_bad_summary = String::new();
    if let Some(snap) = &snapshot {
        if let Some(text) = snap.get("driver_text").and_then(|v| v.as_str()) {
            let inventory = parse_pnputil_enum(text);
            let blocklist = KnownBadDriver::active().await.unwrap_or_default();
            let hits = KnownBadDriver::match_inventory(&blocklist, &inventory);
            known_bad_summary = if hits.is_empty() {
                format!("{} packages, no blocklist hits", inventory.len())
            } else {
                hits.iter()
                    .map(|h| {
                        format!(
                            "{} {} — {} (fix: {})",
                            h.driver.key(),
                            h.driver.driver_version,
                            h.entry.symptom,
                            h.entry.fix
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            };
        }
    }

    // 4. Detached WinDbg batch analysis, polled until done (hook ingests signatures).
    let has_dumps = survey
        .as_ref()
        .and_then(|s| s.get("data").unwrap_or(s).get("minidump_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        > 0;
    let mut crash_summary = "no minidumps on disk".to_string();
    if has_dumps {
        push_shared_notice(&cs, "Triage 4/4: WinDbg batch analysis (detached)".to_string());
        let _ = call_tool(cmd_tx, DUMP_DECODE_PLUGIN_ID, "run_analyze_batch", 60).await;
        let mut done = false;
        for _ in 0..12 {
            tokio::time::sleep(Duration::from_secs(25)).await;
            if let Some(batch) = call_tool(cmd_tx, DUMP_DECODE_PLUGIN_ID, "read_batch", 45).await {
                if payload_status(&batch).as_deref() == Some("done") {
                    done = true;
                    break;
                }
            }
        }
        if done {
            // The receive-path hook ingests asynchronously; give it a moment.
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if !crash_intel_hooks::latest_ingests(&cs).is_empty() {
                    break;
                }
            }
            let ingests = crash_intel_hooks::latest_ingests(&cs);
            crash_summary = if ingests.is_empty() {
                "batch done, no parseable signatures".to_string()
            } else {
                ingests
                    .iter()
                    .map(|i| {
                        let verdict = i
                            .verdicts
                            .first()
                            .map(|v| format!(" KNOWN: {} (fix: {})", v.verdict, v.fix))
                            .unwrap_or_default();
                        format!(
                            "{} {} x{} on {} machine(s){}",
                            i.signature.bugcheck_code,
                            i.signature.module,
                            i.signature.sighting_count,
                            i.signature.machines.len(),
                            verdict
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            };
        } else {
            crash_summary = "batch analysis still running (check Fleet Intel later)".to_string();
        }
    } else {
        push_shared_notice(&cs, "Triage 4/4: skipped (no minidumps)".to_string());
    }

    let summary = format!(
        "Triage summary — survey: {survey_summary} | crashes: {crash_summary} | drivers: {known_bad_summary}"
    );
    log_entry(
        sk,
        DiagnosticCategory::Note,
        "Autopilot: triage summary",
        summary.clone(),
        None,
    )
    .await;

    if with_ai {
        push_shared_notice(&cs, "Drafting AI triage verdict…".to_string());
        match draft_ai_verdict(&cs, sk, &summary, &oldest_drivers).await {
            Ok(verdict) if !verdict.trim().is_empty() => {
                log_entry(
                    sk,
                    DiagnosticCategory::Recommendation,
                    "Autopilot: AI triage draft",
                    verdict.clone(),
                    None,
                )
                .await;
                push_shared_notice(&cs, format!("AI triage draft: {verdict}"));
            }
            Ok(_) => push_shared_notice(&cs, "AI returned an empty draft".to_string()),
            Err(e) => push_shared_notice(&cs, format!("AI draft unavailable: {e}")),
        }
    }

    Ok(summary)
}

/// One Claude Code turn drafting a verdict from the gathered triage material.
async fn draft_ai_verdict(
    connection_string: &str,
    session_key: Option<&str>,
    summary: &str,
    oldest_drivers: &str,
) -> anyhow::Result<String> {
    let prompt = format!(
        "Intake triage just ran on connected client '{connection_string}'\
         {session}. Gathered: {summary}. Oldest third-party drivers: {drivers}. \
         Use crash_intel_search / crash_intel_signature for prior fleet verdicts on any \
         signature mentioned, and get_diagnostic_session for the full entry data. Then write a \
         short intake verdict draft for the tech: most likely root cause, confidence, and the \
         first two bench actions. If a signature has a recorded fleet verdict, lead with it.",
        session = session_key
            .map(|k| format!(" (diagnostic session {k})"))
            .unwrap_or_default(),
        drivers = if oldest_drivers.is_empty() { "n/a" } else { oldest_drivers },
    );

    let (tx, rx) = crossbeam::channel::unbounded::<ChatMessage>();
    let session = ClaudeCodeSession::new();
    session.send(
        prompt,
        Some(connection_string.to_string()),
        "autopilot-triage".to_string(),
        tx,
    );

    tokio::task::spawn_blocking(move || {
        let mut text = String::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(300)) {
                Ok(msg) => match msg.content {
                    ChatMessageType::Text(t) => {
                        if !t.starts_with(crate::ai::claude_code::TOOL_PREFIX) {
                            text.push_str(&t);
                        }
                    }
                    ChatMessageType::Error(e) => anyhow::bail!("claude error: {e}"),
                    ChatMessageType::Done => break,
                    _ => {}
                },
                Err(_) => anyhow::bail!("claude timed out"),
            }
        }
        Ok(text.trim().to_string())
    })
    .await?
}

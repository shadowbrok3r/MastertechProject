#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
//! Turns ZeroClaw agent activity into Mastertech notifications so unattended
//! pipeline work surfaces in the app instead of only in the agent host's logs.
//!
//! Polls `/api/events/history` rather than subscribing to `/api/events`: the
//! SSE stream accepts the connection and then only emits keepalive comments,
//! while the history ring buffer carries the real frames.

use std::collections::VecDeque;

use database::schema::notification::Notification;
use database::schema::RecordId;

use crate::{PlatformSpawner, Spawner};

const POLL_SECS: u64 = 10;
/// Ring buffer upstream holds ~20 entries; remember more than that so a slow
/// poll cycle can't replay something already notified.
const SEEN_CAP: usize = 256;

/// Tools whose invocation is worth a notification; everything else is noise.
fn notable(tool: &str) -> Option<&'static str> {
    match tool.rsplit("__").next().unwrap_or(tool) {
        "claude_code" | "claude_code_runner" => Some("Claude session started"),
        "minidump_analyze" => Some("Crash dump analyzed"),
        "crash_verdict_record" => Some("Crash verdict recorded"),
        "create_ai_task" | "add_ai_task_steps" => Some("AI task updated"),
        "close_diagnostic_session" => Some("Diagnostic session closed"),
        "create_diagnostic_session" => Some("Diagnostic session opened"),
        "scripts_run_remote" | "scripts_run_stress_suite_remote" => Some("Remote script run"),
        _ => None,
    }
}

/// Stable identity for an event so repeated polls don't re-notify.
fn key_of(v: &serde_json::Value) -> String {
    format!(
        "{}|{}|{}|{}",
        v["timestamp"].as_str().unwrap_or_default(),
        v["type"].as_str().unwrap_or_default(),
        v["turn_id"].as_str().unwrap_or_default(),
        v["tool"].as_str().unwrap_or_default()
    )
}

/// One event -> an optional (notification type, description).
fn to_notification(v: &serde_json::Value) -> Option<(String, String)> {
    let kind = v["type"].as_str().unwrap_or_default();
    let agent = v["agent_alias"].as_str().unwrap_or("agent");
    if kind.contains("error") {
        let msg = v["message"].as_str().or_else(|| v["error"].as_str()).unwrap_or("unknown error");
        return Some(("ZeroClaw Error".into(), format!("{agent}: {msg}")));
    }
    // tool_call is the completion event; tool_call_start would double-notify.
    if kind == "tool_call" {
        let tool = v["tool"].as_str()?;
        let failed = v["success"].as_bool() == Some(false);
        // Any failure is worth surfacing, even for tools too chatty to report on success.
        let label = match notable(tool) {
            Some(l) => l,
            None if failed => "Tool call failed",
            None => return None,
        };
        let mut detail = format!("{label} ({tool}) on {agent}");
        if failed {
            detail.push_str(" - FAILED");
        }
        if let Some(ms) = v["duration_ms"].as_u64() {
            detail.push_str(&format!(" in {ms}ms"));
        }
        let kind = if failed { "ZeroClaw Error" } else { "ZeroClaw Activity" };
        return Some((kind.into(), detail));
    }
    None
}

async fn poll_once(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    user: &RecordId,
    seen: &mut VecDeque<String>,
) -> anyhow::Result<()> {
    let body: serde_json::Value = client
        .get(format!("{url}/api/events/history"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let Some(events) = body["events"].as_array() else { return Ok(()) };
    for ev in events {
        let key = key_of(ev);
        if seen.contains(&key) {
            continue;
        }
        seen.push_back(key);
        while seen.len() > SEEN_CAP {
            seen.pop_front();
        }
        let Some((kind, description)) = to_notification(ev) else { continue };
        let mut n = Notification { user: user.clone(), ..Default::default() };
        n.set_type(kind).set_description(description);
        if let Err(e) = n.create().await {
            log::warn!("zeroclaw_events: notification write failed: {e}");
        }
    }
    Ok(())
}

/// Writes a notification so watcher state is visible without console access.
async fn announce(user: &RecordId, kind: &str, detail: String) {
    let mut n = Notification { user: user.clone(), ..Default::default() };
    n.set_type(kind).set_description(detail);
    if let Err(e) = n.create().await {
        log::warn!("zeroclaw_events: announce failed: {e}");
    }
}

/// Spawns the poller. Announces its own state either way.
pub fn spawn(user: RecordId) {
    let Some((url, token)) = crate::ai::mcp_chat::zeroclaw_gateway() else {
        let path = crate::ai::mcp_chat::zeroclaw_config_path();
        log::warn!("zeroclaw_events: gateway not configured - write {}", path.display());
        PlatformSpawner::spawn(async move {
            announce(
                &user,
                "ZeroClaw Error",
                format!("Watcher inactive: no gateway configured ({})", path.display()),
            )
            .await;
        });
        return;
    };
    PlatformSpawner::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                log::error!("zeroclaw_events: client build failed: {e}");
                return;
            }
        };
        // Prime the seen-set so startup doesn't replay the whole buffer as new.
        let mut seen: VecDeque<String> = VecDeque::new();
        if let Ok(body) = client
            .get(format!("{url}/api/events/history"))
            .bearer_auth(&token)
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            if let Ok(v) = body.json::<serde_json::Value>().await {
                if let Some(events) = v["events"].as_array() {
                    seen.extend(events.iter().map(key_of));
                }
            }
        }
        log::info!("zeroclaw_events: polling {url}/api/events/history every {POLL_SECS}s");
        announce(
            &user,
            "ZeroClaw Activity",
            format!("Watcher started - polling {url} every {POLL_SECS}s ({} events primed)", seen.len()),
        )
        .await;
        loop {
            if let Err(e) = poll_once(&client, &url, &token, &user, &mut seen).await {
                log::warn!("zeroclaw_events: poll failed: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(POLL_SECS)).await;
        }
    });
}

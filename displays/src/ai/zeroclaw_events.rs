#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
//! Turns ZeroClaw agent activity into Mastertech notifications so unattended
//! pipeline work surfaces in the app instead of only in the agent host's logs.
//!
//! Polls `/api/events/history` rather than subscribing to `/api/events`: the
//! SSE stream accepts the connection and then only emits keepalive comments,
//! while the history ring buffer carries the real frames. The endpoint honours
//! no window parameter and replays its whole buffer on every poll.

use std::collections::HashSet;

use database::schema::notification::Notification;
use database::schema::RecordId;
use jiff::Timestamp;

use crate::{PlatformSpawner, Spawner};

const POLL_SECS: u64 = 10;

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

/// Distinguishes events sharing one instant.
fn key_of(v: &serde_json::Value) -> String {
    format!(
        "{}|{}|{}",
        v["type"].as_str().unwrap_or_default(),
        v["turn_id"].as_str().unwrap_or_default(),
        v["tool"].as_str().unwrap_or_default()
    )
}

/// Event instant; unparseable timestamps cannot be ordered and are skipped.
fn event_ts(v: &serde_json::Value) -> Option<Timestamp> {
    v["timestamp"].as_str()?.parse().ok()
}

/// Newest handled event instant plus the keys sharing it.
#[derive(Default)]
struct Watermark {
    at: Option<Timestamp>,
    keys_at: HashSet<String>,
}

impl Watermark {
    /// Records the event and reports whether it was not already handled.
    fn admit(&mut self, ts: Timestamp, key: String) -> bool {
        match self.at {
            Some(at) if ts < at => false,
            Some(at) if ts == at => self.keys_at.insert(key),
            _ => {
                self.at = Some(ts);
                self.keys_at.clear();
                self.keys_at.insert(key);
                true
            }
        }
    }

    /// Marks every event as handled without notifying.
    fn prime(&mut self, events: &[serde_json::Value]) {
        for ev in events {
            if let Some(ts) = event_ts(ev) {
                self.admit(ts, key_of(ev));
            }
        }
    }
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
    mark: &mut Watermark,
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
        let Some(ts) = event_ts(ev) else { continue };
        if !mark.admit(ts, key_of(ev)) {
            continue;
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
        log::warn!(
            "zeroclaw_events: gateway not configured - set ZEROCLAW_GATEWAY_URL/_TOKEN in .env and \
             rebuild, or write {}",
            path.display()
        );
        PlatformSpawner::spawn(async move {
            announce(
                &user,
                "ZeroClaw Error",
                "Watcher inactive: no ZeroClaw gateway configured in this build".to_string(),
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
        // Startup must not replay the buffer as new.
        let mut mark = Watermark::default();
        if let Ok(body) = client
            .get(format!("{url}/api/events/history"))
            .bearer_auth(&token)
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            if let Ok(v) = body.json::<serde_json::Value>().await {
                if let Some(events) = v["events"].as_array() {
                    mark.prime(events);
                }
            }
        }
        log::debug!("zeroclaw_events: polling {url}/api/events/history every {POLL_SECS}s");
        let caught_up = mark.at.map(|t| t.to_string()).unwrap_or_else(|| "nothing".into());
        announce(
            &user,
            "ZeroClaw Activity",
            format!("Watcher started - polling {url} every {POLL_SECS}s, caught up to {caught_up}"),
        )
        .await;
        // Only the first failure of a run warns; the rest are silent until a poll succeeds.
        let mut failing = false;
        loop {
            match poll_once(&client, &url, &token, &user, &mut mark).await {
                Err(e) if failing => log::debug!("zeroclaw_events: poll failed: {e}"),
                Err(e) => {
                    failing = true;
                    log::warn!("zeroclaw_events: poll failed: {e}");
                }
                Ok(()) if failing => {
                    failing = false;
                    log::info!("zeroclaw_events: polling recovered");
                }
                Ok(()) => {}
            }
            tokio::time::sleep(std::time::Duration::from_secs(POLL_SECS)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(ts: &str, tool: &str) -> serde_json::Value {
        json!({"timestamp": ts, "type": "tool_call", "turn_id": "t1", "tool": tool})
    }

    /// Number of events the watermark treats as new.
    fn admit_all(mark: &mut Watermark, buf: &[serde_json::Value]) -> usize {
        buf.iter().filter(|e| mark.admit(event_ts(e).unwrap(), key_of(e))).count()
    }

    #[test]
    fn replayed_buffer_admits_each_event_once() {
        let buf = vec![ev("2026-08-07T16:00:00Z", "claude_code"), ev("2026-08-07T16:01:00Z", "claude_code")];
        let mut mark = Watermark::default();
        assert_eq!(admit_all(&mut mark, &buf), 2);
        assert_eq!(admit_all(&mut mark, &buf), 0);
        assert_eq!(admit_all(&mut mark, &buf), 0);
    }

    #[test]
    fn buffer_larger_than_any_cap_still_dedups() {
        let buf: Vec<_> = (0..500)
            .map(|i| ev(&format!("2026-08-07T16:{:02}:{:02}Z", i / 60, i % 60), "claude_code"))
            .collect();
        let mut mark = Watermark::default();
        assert_eq!(admit_all(&mut mark, &buf), 500);
        assert_eq!(admit_all(&mut mark, &buf), 0);
    }

    #[test]
    fn siblings_at_one_instant_are_both_admitted() {
        let buf = vec![ev("2026-08-07T16:00:00Z", "claude_code"), ev("2026-08-07T16:00:00Z", "minidump_analyze")];
        let mut mark = Watermark::default();
        assert_eq!(admit_all(&mut mark, &buf), 2);
        assert_eq!(admit_all(&mut mark, &buf), 0);
    }

    #[test]
    fn events_after_the_watermark_are_admitted() {
        let mut mark = Watermark::default();
        let buf = vec![ev("2026-08-07T16:00:00Z", "claude_code")];
        assert_eq!(admit_all(&mut mark, &buf), 1);
        let grown = vec![buf[0].clone(), ev("2026-08-07T16:05:00Z", "claude_code")];
        assert_eq!(admit_all(&mut mark, &grown), 1);
        assert_eq!(admit_all(&mut mark, &grown), 0);
    }

    #[test]
    fn prime_suppresses_the_existing_buffer() {
        let buf = vec![ev("2026-08-07T16:00:00Z", "claude_code"), ev("2026-08-07T16:01:00Z", "claude_code")];
        let mut mark = Watermark::default();
        mark.prime(&buf);
        assert_eq!(admit_all(&mut mark, &buf), 0);
    }
}

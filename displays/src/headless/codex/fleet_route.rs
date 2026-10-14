//! Stamps live agent threads with the node, route, quant and model of their latest request, read from the fleet relay.

use std::collections::HashMap;
use std::time::Duration;

use database::schema::{AgentThread, Datetime, FleetStamp, RecordIdExt};
use serde::Deserialize;

const POLL: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Below the relay's 10 s keep-alive timeout.
const POOL_IDLE: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Most thread ids the relay accepts in one request.
const BATCH: usize = 200;
const THREAD_ID_MAX: usize = 64;

/// Relay address and bearer token from `MTECH_FLEET_RELAY_URL` and `MTECH_FLEET_RELAY_TOKEN`.
pub struct Relay {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl Relay {
    /// `None` when either variable is unset.
    pub fn from_env() -> Option<Self> {
        let base = super::env_trimmed("MTECH_FLEET_RELAY_URL")?;
        let token = super::env_trimmed("MTECH_FLEET_RELAY_TOKEN")?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .pool_idle_timeout(POOL_IDLE)
            .build()
            .ok()?;
        Some(Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            http,
        })
    }

    /// Latest stamp per codex thread id for `threads`, one request per batch.
    async fn routes(&self, threads: &[String]) -> anyhow::Result<HashMap<String, FleetStamp>> {
        let mut stamps = HashMap::new();
        for url in routes_urls(&self.base, threads) {
            let body = self
                .http
                .get(url)
                .bearer_auth(&self.token)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(reqwest::Error::without_url)?
                .text()
                .await
                .map_err(reqwest::Error::without_url)?;
            stamps.extend(parse_routes(&body)?);
        }
        Ok(stamps)
    }
}

/// Starts the poller when the relay is configured; logs either way.
pub fn spawn() {
    match Relay::from_env() {
        Some(relay) => {
            log::info!("codex: fleet route stamps on, relay {}", relay.base);
            tokio::spawn(poll_loop(relay));
        }
        None => log::info!(
            "codex: MTECH_FLEET_RELAY_URL/MTECH_FLEET_RELAY_TOKEN unset; fleet route stamps off"
        ),
    }
}

async fn poll_loop(relay: Relay) {
    let mut delay = POLL;
    let mut failures = 0u32;
    loop {
        tokio::time::sleep(delay).await;
        let threads = match AgentThread::fleet_watch_list().await {
            Ok(t) => t,
            Err(e) => {
                log::debug!("codex: fleet route: could not list threads: {e}");
                continue;
            }
        };
        let ids = thread_ids(&threads);
        if ids.is_empty() {
            continue;
        }
        match relay.routes(&ids).await {
            Ok(stamps) => {
                if failures > 0 {
                    log::info!("codex: fleet relay answering again after {failures} failed polls");
                }
                failures = 0;
                delay = POLL;
                write_changed(&threads, &stamps).await;
            }
            Err(e) => {
                if failures == 0 {
                    log::warn!("codex: fleet relay unavailable, backing off: {e}");
                }
                failures = failures.saturating_add(1);
                delay = next_delay(delay);
            }
        }
    }
}

/// Writes the stamp of each thread whose relay entry differs from what it stores.
async fn write_changed(threads: &[AgentThread], stamps: &HashMap<String, FleetStamp>) {
    for thread in threads {
        let Some(seen) = thread
            .codex_thread_id
            .as_deref()
            .and_then(|id| stamps.get(id))
        else {
            continue;
        };
        let Some(next) = seen.update_over(&thread.fleet_stamp()) else {
            continue;
        };
        if let Err(e) = AgentThread::set_fleet_stamp(&thread.id, &next).await {
            log::warn!(
                "codex: fleet route stamp for thread {}: {e}",
                thread.id.key_string()
            );
        }
    }
}

/// Distinct, sorted codex thread ids the relay accepts.
fn thread_ids(threads: &[AgentThread]) -> Vec<String> {
    let mut ids: Vec<String> = threads
        .iter()
        .filter_map(|t| t.codex_thread_id.as_deref())
        .filter(|id| valid_thread_id(id))
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// `[A-Za-z0-9-]{1,64}`.
fn valid_thread_id(id: &str) -> bool {
    (1..=THREAD_ID_MAX).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// One `/v1/routes` URL per batch of ids.
fn routes_urls(base: &str, ids: &[String]) -> Vec<String> {
    ids.chunks(BATCH)
        .map(|chunk| format!("{base}/v1/routes?threads={}", chunk.join(",")))
        .collect()
}

/// Doubles the poll delay up to [`MAX_BACKOFF`].
fn next_delay(current: Duration) -> Duration {
    (current * 2).min(MAX_BACKOFF)
}

#[derive(Debug, Deserialize)]
struct RoutesBody {
    #[serde(default)]
    threads: Vec<RouteEntry>,
}

#[derive(Debug, Deserialize)]
struct RouteEntry {
    thread: String,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    route: Option<String>,
    #[serde(default)]
    served_at: Option<String>,
    #[serde(default)]
    quant: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// Stamps by codex thread id from a `/v1/routes` body; entries without a node are dropped.
fn parse_routes(body: &str) -> anyhow::Result<HashMap<String, FleetStamp>> {
    let parsed: RoutesBody = serde_json::from_str(body)?;
    Ok(parsed
        .threads
        .into_iter()
        .filter_map(|e| {
            let node = non_empty(e.node)?;
            let stamp = FleetStamp {
                node: Some(node),
                route: non_empty(e.route),
                quant: non_empty(e.quant),
                model: non_empty(e.model),
                served_at: e.served_at.as_deref().and_then(parse_rfc3339),
            };
            Some((e.thread, stamp))
        })
        .collect())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_rfc3339(raw: &str) -> Option<Datetime> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|d| Datetime::from(d.with_timezone(&chrono::Utc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
        "checked_at": "2026-10-14T20:01:02Z",
        "threads": [
            {"thread": "01a13bab-d0fe-7511-8dc5-4166e6005de8", "node": "node-7900", "route": "pinned",
             "served_at": "2026-10-14T19:33:49.123456789Z", "quant": "Q8_0", "model": "Qwen3.8-27B-Q8_0.gguf"},
            {"thread": "01a13b91-0c66-7d52-b071-306b8cc3f350", "node": "r9700", "route": "compaction",
             "served_at": "2026-10-14T19:28:46+00:00", "quant": null, "model": null},
            {"thread": "01a13741-490f-7961-a2e8-052af2a929f8", "node": null, "route": "pinned",
             "served_at": "2026-10-14T18:04:00Z", "quant": "Q6_K_P", "model": "x.gguf"},
            {"thread": "01a127d3-0000-0000-0000-000000000000", "node": "node-128", "route": "",
             "served_at": "yesterday", "quant": " ", "model": "Qwen3.8-27B-HauhauCS-Aggressive-Q6_K_P.gguf",
             "extra": 1}
        ]
    }"#;

    #[test]
    fn relay_entries_parse_into_stamps() {
        let stamps = parse_routes(BODY).expect("parses");
        assert_eq!(stamps.len(), 3, "an entry without a node is dropped");

        let pinned = &stamps["01a13bab-d0fe-7511-8dc5-4166e6005de8"];
        assert_eq!(pinned.node.as_deref(), Some("node-7900"));
        assert_eq!(pinned.route.as_deref(), Some("pinned"));
        assert_eq!(pinned.quant.as_deref(), Some("Q8_0"));
        assert_eq!(pinned.model.as_deref(), Some("Qwen3.8-27B-Q8_0.gguf"));
        let served = chrono::DateTime::<chrono::Utc>::from(pinned.served_at.expect("served_at"));
        assert_eq!(served.timestamp_subsec_nanos(), 123_456_789);

        let compaction = &stamps["01a13b91-0c66-7d52-b071-306b8cc3f350"];
        assert_eq!(
            (compaction.quant.as_deref(), compaction.model.as_deref()),
            (None, None)
        );
        assert_eq!(compaction.served_at, parse_rfc3339("2026-10-14T19:28:46Z"));

        let odd = &stamps["01a127d3-0000-0000-0000-000000000000"];
        assert_eq!(
            (odd.route.as_deref(), odd.quant.as_deref(), odd.served_at),
            (None, None, None)
        );
    }

    #[test]
    fn an_empty_or_broken_body() {
        assert!(
            parse_routes(r#"{"checked_at": "2026-10-14T20:01:02Z"}"#)
                .expect("parses")
                .is_empty()
        );
        assert!(
            parse_routes(r#"{"threads": [{"node": "node-7900"}]}"#).is_err(),
            "an entry needs its thread"
        );
        assert!(parse_routes("<html>502</html>").is_err());
    }

    #[test]
    fn a_relay_entry_matching_the_stored_stamp_writes_nothing() {
        let stamps = parse_routes(BODY).expect("parses");
        let pinned = &stamps["01a13bab-d0fe-7511-8dc5-4166e6005de8"];
        assert_eq!(pinned.update_over(pinned), None);
        let stored = FleetStamp {
            quant: Some("MXFP4".into()),
            ..stamps["01a13b91-0c66-7d52-b071-306b8cc3f350"].clone()
        };
        assert_eq!(
            stamps["01a13b91-0c66-7d52-b071-306b8cc3f350"].update_over(&stored),
            None
        );
        assert!(pinned.update_over(&stored).is_some());
    }

    #[test]
    fn thread_ids_follow_the_relay_grammar() {
        assert!(valid_thread_id("01a13bab-d0fe-7511-8dc5-4166e6005de8"));
        assert!(valid_thread_id(&"a".repeat(64)));
        for bad in ["", "a b", "a,b", "a/b", "a_b", "é", &"a".repeat(65)] {
            assert!(!valid_thread_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn ids_are_batched_two_hundred_per_request() {
        let ids: Vec<String> = (0..401).map(|i| format!("t{i}")).collect();
        let urls = routes_urls("http://192.168.22.7:7431", &ids);
        assert_eq!(urls.len(), 3);
        assert!(urls[0].starts_with("http://192.168.22.7:7431/v1/routes?threads=t0,t1,"));
        assert_eq!(urls[0].matches(',').count(), 199);
        assert_eq!(urls[2], "http://192.168.22.7:7431/v1/routes?threads=t400");
        assert!(routes_urls("http://x", &[]).is_empty());
    }

    #[test]
    fn backoff_doubles_up_to_a_minute() {
        assert_eq!(next_delay(POLL), Duration::from_secs(20));
        assert_eq!(next_delay(Duration::from_secs(40)), MAX_BACKOFF);
        assert_eq!(next_delay(MAX_BACKOFF), MAX_BACKOFF);
    }
}

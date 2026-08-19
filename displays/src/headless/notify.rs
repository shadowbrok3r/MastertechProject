//! Posts Check-in Shelf triage verdicts to Discord.
//!
//! Outbound only, through a channel webhook rather than the bot token: a webhook
//! can post to exactly one channel, so the daemon never holds a credential that
//! could read or write anywhere else in the server.
//!
//! Rows are marked `notified` only after Discord accepts the post, so a failed
//! send retries on the next pass instead of losing the candidate.

use std::time::Duration;

/// How often the sweep results are checked; the sweep itself runs twice a day.
const POLL_SECS: u64 = 300;
/// Default bar for "worth a technician's attention".
const DEFAULT_MIN_SCORE: i64 = 50;
/// Discord rejects messages over 2000 characters.
const MAX_BODY_CHARS: usize = 1800;
const MAX_PER_POST: usize = 5;

struct Candidate {
    service_number: String,
    score: i64,
    reason: String,
    store: Option<String>,
    waiting_open_hours: Option<f64>,
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn min_score() -> i64 {
    env_value("MTECH_DISCORD_MIN_SCORE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MIN_SCORE)
}

/// Unnotified candidates at or above the score bar, best first.
async fn pending(min_score: i64) -> Vec<Candidate> {
    let sql = "SELECT service_number, score, reason, store, waiting_open_hours \
               FROM shelf_candidate WHERE notified != true AND score >= $min \
               ORDER BY score DESC LIMIT $limit";
    let rows: Vec<serde_json::Value> = match database::db()
        .query(sql)
        .bind(("min", min_score))
        .bind(("limit", MAX_PER_POST))
        .await
    {
        Ok(mut res) => res.take(0).unwrap_or_default(),
        Err(e) => {
            log::warn!("notify: shelf query failed: {e}");
            return Vec::new();
        },
    };
    rows.iter()
        .filter_map(|r| {
            let service_number = r.get("service_number")?.as_str()?.to_string();
            Some(Candidate {
                service_number,
                score: r.get("score").and_then(serde_json::Value::as_i64).unwrap_or(0),
                reason: r
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(280)
                    .collect(),
                store: r.get("store").and_then(|v| v.as_str()).map(str::to_string),
                waiting_open_hours: r
                    .get("waiting_open_hours")
                    .and_then(serde_json::Value::as_f64),
            })
        })
        .collect()
}

fn compose(cands: &[Candidate]) -> String {
    let mut out = String::from("**Check-in Shelf — AI-assist candidates**\n");
    for c in cands {
        let store = c.store.as_deref().unwrap_or("-");
        let waited = c
            .waiting_open_hours
            .map(|h| format!("{h:.0}h open"))
            .unwrap_or_else(|| "unknown wait".to_string());
        out.push_str(&format!(
            "\n**#{}** · score {} · {store} · {waited}\n{}\n",
            c.service_number, c.score, c.reason
        ));
        if out.chars().count() > MAX_BODY_CHARS {
            out.push_str("\n(more on the shelf; trimmed)\n");
            break;
        }
    }
    out.push_str("\nReply here when one is plugged in.");
    out.chars().take(MAX_BODY_CHARS + 200).collect()
}

async fn mark_notified(numbers: &[String]) {
    if let Err(e) = database::db()
        .query("UPDATE shelf_candidate SET notified = true WHERE service_number IN $nums")
        .bind(("nums", numbers.to_vec()))
        .await
    {
        log::warn!("notify: could not mark candidates notified: {e}");
    }
}

async fn post_once() {
    let Some(webhook) = env_value("MTECH_DISCORD_WEBHOOK") else { return };
    let cands = pending(min_score()).await;
    if cands.is_empty() {
        return;
    }
    let body = compose(&cands);
    let sent = reqwest::Client::new()
        .post(&webhook)
        .json(&serde_json::json!({ "content": body }))
        .send()
        .await;
    match sent {
        Ok(resp) if resp.status().is_success() => {
            let numbers: Vec<String> = cands.into_iter().map(|c| c.service_number).collect();
            log::info!("notify: posted {} shelf candidate(s) to Discord", numbers.len());
            mark_notified(&numbers).await;
        },
        Ok(resp) => log::warn!("notify: Discord returned {}", resp.status()),
        Err(e) => log::warn!("notify: Discord post failed: {e}"),
    }
}

/// Watches sweep results for the life of the process.
pub fn spawn_shelf_notifier() {
    if env_value("MTECH_DISCORD_WEBHOOK").is_none() {
        log::info!("notify: MTECH_DISCORD_WEBHOOK unset; shelf notifications disabled");
        return;
    }
    log::info!("notify: shelf notifications every {POLL_SECS}s at score >= {}", min_score());
    tokio::spawn(async move {
        loop {
            post_once().await;
            tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
        }
    });
}

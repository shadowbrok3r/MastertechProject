//! ZeroClaw's memory through its gateway REST API, offered to the agent as two
//! tools and used by the broker for a brief at start and a note at close, so a
//! Codex session reads and writes the same store ZeroClaw's own agents use.

use std::time::Duration;

use serde_json::{json, Value};

pub const TOOL_NAMES: [&str; 2] = ["zeroclaw_recall", "zeroclaw_remember"];
/// Memory alias for machine sessions.
pub const MACHINE_AGENT: &str = "diagnostician";
/// Memory alias for a technician's session with no machine in scope.
pub const GENERAL_AGENT: &str = "tech_chat";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const RECALL_LIMIT: usize = 12;
const ENTRY_CHARS: usize = 600;

#[derive(Debug, Clone)]
pub struct ZeroclawMemory {
    base: String,
    token: String,
    http: reqwest::Client,
}

/// One memory entry as the gateway lists it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub category: String,
    pub content: String,
    pub when: String,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl ZeroclawMemory {
    /// `MTECH_ZC_GATEWAY` (else the compiled-in gateway URL) plus `MTECH_ZC_TOKEN`
    /// or `ZEROCLAW_GATEWAY_TOKEN`; `None` without a token.
    pub fn from_env() -> Option<Self> {
        let base = env("MTECH_ZC_GATEWAY")
            .or_else(|| Some(database::ZEROCLAW_GATEWAY_URL.trim().to_string()).filter(|v| !v.is_empty()))?;
        let token = env("MTECH_ZC_TOKEN").or_else(|| env("ZEROCLAW_GATEWAY_TOKEN"))?;
        let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().ok()?;
        Some(Self { base: base.trim_end_matches('/').to_string(), token, http })
    }

    pub fn agent_for(connection_string: &str) -> &'static str {
        if super::is_general(connection_string) { GENERAL_AGENT } else { MACHINE_AGENT }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Hybrid search over the alias's memory; the gateway ranks, we cap.
    pub async fn recall(&self, agent: &str, query: &str) -> anyhow::Result<Vec<Entry>> {
        let body: Value = self
            .http
            .get(format!("{}/api/memory", self.base))
            .bearer_auth(&self.token)
            .query(&[("query", query), ("agent", agent)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let entries = body
            .get("entries")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(entry_from).take(RECALL_LIMIT).collect())
            .unwrap_or_default();
        Ok(entries)
    }

    pub async fn store(&self, agent: &str, key: &str, content: &str, category: &str) -> anyhow::Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/memory", self.base))
            .bearer_auth(&self.token)
            .json(&json!({ "key": key, "content": content, "category": category, "agent": agent }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("gateway {status}: {}", text.chars().take(200).collect::<String>());
        }
        Ok(())
    }
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Object(o) => o.keys().next().cloned().unwrap_or_default(),
        other => other.to_string(),
    }
}

fn entry_from(v: &Value) -> Option<Entry> {
    let content = v.get("content").and_then(Value::as_str)?.trim().to_string();
    if content.is_empty() {
        return None;
    }
    let when = ["timestamp", "created_at", "updated_at"]
        .iter()
        .find_map(|k| v.get(*k))
        .map(text_of)
        .unwrap_or_default();
    Some(Entry {
        key: v.get("key").map(text_of).unwrap_or_default(),
        category: v.get("category").map(text_of).unwrap_or_default().to_lowercase(),
        content,
        when,
    })
}

/// One line per entry, clipped, for a brief or a tool result.
/// Entries whose key or content names one of `needles`, compared case-insensitively; blank needles are ignored.
pub fn entries_about(entries: Vec<Entry>, needles: &[&str]) -> Vec<Entry> {
    let needles: Vec<String> = needles
        .iter()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    entries
        .into_iter()
        .filter(|e| {
            let key = e.key.to_lowercase();
            let content = e.content.to_lowercase();
            needles.iter().any(|n| key.contains(n) || content.contains(n))
        })
        .collect()
}

pub fn render_entries(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|e| {
            let content: String = e.content.chars().take(ENTRY_CHARS).collect();
            let content = content.replace('\n', " ");
            let when = if e.when.is_empty() { String::new() } else { format!(" ({})", e.when.chars().take(10).collect::<String>()) };
            format!("- [{}] {}: {}{}", e.category, e.key, content, when)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn is_memory_tool(name: &str) -> bool {
    TOOL_NAMES.contains(&name)
}

/// `thread/start.dynamicTools` entries for the two memory tools.
pub fn tool_specs() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "name": "zeroclaw_recall",
            "description": "Search this agent's long-term ZeroClaw memory (fleet quirks, prior verdicts, decisions, machine history). Returns the best-matching entries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to look for: a hostname, signature, customer, symptom or topic." }
                },
                "required": ["query"]
            },
            "deferLoading": false,
        }),
        json!({
            "type": "function",
            "name": "zeroclaw_remember",
            "description": "Store a durable conclusion in this agent's ZeroClaw memory so future sessions find it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Stable key such as <hostname>/<topic> or <signature>/verdict." },
                    "content": { "type": "string", "description": "The fact or conclusion, with the evidence that supports it." },
                    "category": { "type": "string", "enum": ["core", "daily", "conversation"], "description": "core for lasting facts (default), daily for session notes." }
                },
                "required": ["key", "content"]
            },
            "deferLoading": false,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, content: &str) -> Entry {
        Entry { key: key.into(), category: "core".into(), content: content.into(), when: String::new() }
    }

    #[test]
    fn a_brief_keeps_only_entries_about_this_machine_or_order() {
        let entries = vec![
            entry("DESKTOP-JFAT75B/bronze-cert-2026-09-25", "Cert: Bronze PASSED clean"),
            entry("desktop-3lf8cbd/onedrive", "Deny ACE removed from the OneDrive root"),
            entry("fleet/verdict", "SO 2155370 was a tuneup"),
        ];
        let kept = entries_about(entries, &["DESKTOP-3LF8CBD", "2155370", " "]);
        let keys: Vec<&str> = kept.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["desktop-3lf8cbd/onedrive", "fleet/verdict"]);
        assert!(entries_about(vec![entry("a", "b")], &["", "  "]).is_empty());
    }

    #[test]
    fn entries_read_loosely_and_render_one_line_each() {
        let raw = json!({ "entries": [
            { "key": "SM3/audio", "content": "Senary codec\nreinstalls useless", "category": "core", "timestamp": "2026-08-01T10:00:00Z" },
            { "key": "empty", "content": "   " },
            { "key": "obj", "content": "x", "category": { "custom": "fleet" } }
        ]});
        let entries: Vec<Entry> = raw["entries"].as_array().unwrap().iter().filter_map(entry_from).collect();
        assert_eq!(entries.len(), 2);
        let text = render_entries(&entries);
        assert!(text.starts_with("- [core] SM3/audio: Senary codec reinstalls useless (2026-08-01)"));
        assert!(text.contains("- [custom] obj: x"));
    }

    #[test]
    fn the_general_session_uses_the_tech_chat_alias() {
        assert_eq!(ZeroclawMemory::agent_for("general:logan@x"), GENERAL_AGENT);
        assert_eq!(ZeroclawMemory::agent_for("DESKTOP-1:abc"), MACHINE_AGENT);
        assert!(is_memory_tool("zeroclaw_recall") && !is_memory_tool("query_surrealdb"));
    }
}

#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
//! Reconstructs ZeroClaw agent turns into readable transcripts.
//!
//! `/api/logs` carries the conversation content keyed by `trace_id`, while
//! `/api/events/history` carries the same turns keyed by `turn_id` and is the
//! only stream naming the agent. Both ids hold the same value.

use std::collections::HashMap;

use serde_json::Value;

/// One line of a transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Prompt { text: String, truncated: bool },
    Response { text: String },
    ToolCall { tool: String, arguments: String },
    ToolResult { tool: String, output: String, error: Option<String> },
    Final { text: String },
}

impl Entry {
    pub fn label(&self) -> &'static str {
        match self {
            Entry::Prompt { .. } => "prompt",
            Entry::Response { .. } => "response",
            Entry::ToolCall { .. } => "tool call",
            Entry::ToolResult { error: Some(_), .. } => "tool failed",
            Entry::ToolResult { .. } => "tool result",
            Entry::Final { .. } => "final answer",
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Entry::ToolResult { error: Some(_), .. })
    }
}

/// One agent turn, newest entry last.
#[derive(Debug, Clone, Default)]
pub struct AgentSession {
    pub trace_id: String,
    pub agent: String,
    pub model: String,
    pub started: String,
    pub ended: String,
    pub entries: Vec<(String, Entry)>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub failures: usize,
}

impl AgentSession {
    pub fn short_id(&self) -> String {
        self.trace_id.chars().take(8).collect()
    }

    pub fn tool_calls(&self) -> usize {
        self.entries.iter().filter(|(_, e)| matches!(e, Entry::ToolCall { .. })).count()
    }

    /// Final answer if the turn produced one, else its last response.
    pub fn outcome(&self) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find_map(|(_, e)| match e {
                Entry::Final { text } | Entry::Response { text } => Some(text.as_str()),
                _ => None,
            })
    }
}

fn attr<'a>(ev: &'a Value, key: &str) -> Option<&'a Value> {
    ev.get("attributes")?.get(key)
}

/// Attribute as text; non-string values render as compact JSON.
fn attr_str(ev: &Value, key: &str) -> Option<String> {
    match attr(ev, key)? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn entry_of(ev: &Value) -> Option<Entry> {
    match ev["message"].as_str()? {
        "llm_request" => Some(Entry::Prompt {
            text: attr_str(ev, "request_messages")?,
            truncated: attr(ev, "request_messages_truncated").and_then(Value::as_bool).unwrap_or(false),
        }),
        "llm_response" => Some(Entry::Response { text: attr_str(ev, "raw_response")? }),
        "tool_call_start" => Some(Entry::ToolCall {
            tool: attr_str(ev, "tool")?,
            arguments: attr_str(ev, "arguments").unwrap_or_default(),
        }),
        "tool_call_result" => Some(Entry::ToolResult {
            tool: attr_str(ev, "tool")?,
            output: attr_str(ev, "output").unwrap_or_default(),
            error: attr_str(ev, "error_reason").filter(|e| !e.trim().is_empty()),
        }),
        "turn_final_response" => Some(Entry::Final { text: attr_str(ev, "text")? }),
        _ => None,
    }
}

/// Agent name per turn; the log stream omits it.
pub fn agents_by_turn(history: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(events) = history["events"].as_array() else { return out };
    for ev in events {
        let (Some(turn), Some(agent)) = (ev["turn_id"].as_str(), ev["agent_alias"].as_str()) else {
            continue;
        };
        out.entry(turn.to_string()).or_insert_with(|| agent.to_string());
    }
    out
}

/// Groups log events into transcripts, newest turn first.
pub fn sessions_from(logs: &Value, agents: &HashMap<String, String>) -> Vec<AgentSession> {
    let Some(events) = logs["events"].as_array() else { return Vec::new() };
    let mut by_trace: HashMap<String, AgentSession> = HashMap::new();
    for ev in events {
        let Some(trace) = ev["trace_id"].as_str().filter(|s| !s.is_empty()) else { continue };
        let ts = ev["@timestamp"].as_str().unwrap_or_default().to_string();
        let s = by_trace.entry(trace.to_string()).or_insert_with(|| AgentSession {
            trace_id: trace.to_string(),
            agent: agents.get(trace).cloned().unwrap_or_else(|| "agent".to_string()),
            ..Default::default()
        });
        if s.model.is_empty() {
            if let Some(m) = attr_str(ev, "model") {
                s.model = m;
            }
        }
        s.input_tokens += attr(ev, "input_tokens").and_then(Value::as_u64).unwrap_or(0);
        s.output_tokens += attr(ev, "output_tokens").and_then(Value::as_u64).unwrap_or(0);
        s.cost_usd += attr(ev, "cost_usd").and_then(Value::as_f64).unwrap_or(0.0);
        if s.started.is_empty() || ts < s.started {
            s.started = ts.clone();
        }
        if ts > s.ended {
            s.ended = ts.clone();
        }
        if let Some(entry) = entry_of(ev) {
            if entry.is_failure() {
                s.failures += 1;
            }
            s.entries.push((ts, entry));
        }
    }
    let mut out: Vec<AgentSession> = by_trace.into_values().collect();
    for s in &mut out {
        s.entries.sort_by(|a, b| a.0.cmp(&b.0));
    }
    out.sort_by(|a, b| b.ended.cmp(&a.ended));
    out
}

async fn get(client: &reqwest::Client, url: &str, token: &str, path: &str) -> anyhow::Result<Value> {
    Ok(client
        .get(format!("{url}{path}"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Pulls both streams and joins them into transcripts.
pub async fn fetch() -> anyhow::Result<Vec<AgentSession>> {
    let Some((url, token)) = crate::ai::mcp_chat::zeroclaw_gateway() else {
        anyhow::bail!(
            "No ZeroClaw gateway configured - set ZEROCLAW_GATEWAY_URL/_TOKEN in .env and rebuild, \
             or write {}",
            crate::ai::mcp_chat::zeroclaw_config_path().display()
        );
    };
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()?;
    let logs = get(&client, &url, &token, "/api/logs").await?;
    let agents = match get(&client, &url, &token, "/api/events/history").await {
        Ok(h) => agents_by_turn(&h),
        Err(e) => {
            log::warn!("zeroclaw_sessions: agent names unavailable: {e}");
            HashMap::new()
        }
    };
    Ok(sessions_from(&logs, &agents))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log_ev(trace: &str, ts: &str, message: &str, attrs: Value) -> Value {
        json!({"trace_id": trace, "@timestamp": ts, "message": message, "attributes": attrs})
    }

    #[test]
    fn groups_by_trace_and_orders_entries() {
        let logs = json!({"events": [
            log_ev("t2", "2026-08-07T16:05:00Z", "llm_response", json!({"raw_response": "second turn"})),
            log_ev("t1", "2026-08-07T16:00:02Z", "tool_call_start", json!({"tool": "claude_code", "arguments": "{}"})),
            log_ev("t1", "2026-08-07T16:00:01Z", "llm_request", json!({"request_messages": "hello", "model": "qwen"})),
        ]});
        let sessions = sessions_from(&logs, &HashMap::new());
        assert_eq!(sessions.len(), 2);
        // Newest turn first.
        assert_eq!(sessions[0].trace_id, "t2");
        let t1 = &sessions[1];
        assert_eq!(t1.model, "qwen");
        assert_eq!(t1.entries.len(), 2);
        assert!(matches!(t1.entries[0].1, Entry::Prompt { .. }));
        assert!(matches!(t1.entries[1].1, Entry::ToolCall { .. }));
        assert_eq!(t1.started, "2026-08-07T16:00:01Z");
        assert_eq!(t1.ended, "2026-08-07T16:00:02Z");
    }

    #[test]
    fn joins_agent_name_from_history() {
        let history = json!({"events": [{"turn_id": "t1", "agent_alias": "sweeper"}]});
        let agents = agents_by_turn(&history);
        let logs = json!({"events": [log_ev("t1", "2026-08-07T16:00:00Z", "llm_request", json!({"request_messages": "x"}))]});
        assert_eq!(sessions_from(&logs, &agents)[0].agent, "sweeper");
    }

    #[test]
    fn counts_failures_and_totals() {
        let logs = json!({"events": [
            log_ev("t1", "2026-08-07T16:00:00Z", "tool_call_result", json!({"tool": "a", "output": "ok", "error_reason": ""})),
            log_ev("t1", "2026-08-07T16:00:01Z", "tool_call_result", json!({"tool": "b", "output": "", "error_reason": "denied"})),
            log_ev("t1", "2026-08-07T16:00:02Z", "llm_response", json!({"raw_response": "done", "input_tokens": 10, "output_tokens": 5, "cost_usd": 0.25})),
        ]});
        let s = &sessions_from(&logs, &HashMap::new())[0];
        assert_eq!(s.failures, 1);
        assert_eq!(s.input_tokens, 10);
        assert_eq!(s.output_tokens, 5);
        assert!((s.cost_usd - 0.25).abs() < f64::EPSILON);
        // An empty error_reason is not a failure.
        assert!(!s.entries[0].1.is_failure());
    }

    #[test]
    fn outcome_prefers_the_final_answer() {
        let logs = json!({"events": [
            log_ev("t1", "2026-08-07T16:00:00Z", "llm_response", json!({"raw_response": "thinking"})),
            log_ev("t1", "2026-08-07T16:00:01Z", "turn_final_response", json!({"text": "the answer"})),
        ]});
        assert_eq!(sessions_from(&logs, &HashMap::new())[0].outcome(), Some("the answer"));
    }

    #[test]
    fn events_without_a_trace_are_dropped() {
        let logs = json!({"events": [
            json!({"@timestamp": "2026-08-07T16:00:00Z", "message": "Memory initialized", "attributes": {}}),
            log_ev("", "2026-08-07T16:00:01Z", "llm_response", json!({"raw_response": "x"})),
        ]});
        assert!(sessions_from(&logs, &HashMap::new()).is_empty());
    }
}

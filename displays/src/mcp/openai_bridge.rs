#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]

use anyhow::Result;
// (StreamingExt not currently needed with Responses API single shot)
use serde_json;
use std::collections::{VecDeque, HashSet};
use crossbeam::channel::Sender as CrossbeamSender;

use crate::{mcp::mcp::ShellType, openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs,
        ChatCompletionTool,
    },
    Client as OpenAIClient,
}};
use crate::openai::types::responses::{CreateResponse, Input, TextConfig, TextResponseFormat, ResponseFormatJsonSchema};
use futures::StreamExt; // for bytes_stream next() on reqwest Response

// (legacy) use rmcp::model::CallToolRequestParam; // removed for now

/// A bridge session that connects OpenAI Chat Completions to an MCP server over TCP.
pub struct OpenAiMcpSession {
    pub oa_client: OpenAIClient<OpenAIConfig>,
    pub model: String,
    #[allow(dead_code)]
    mcp_addr: String,
    #[allow(dead_code)]
    openai_tools: Vec<ChatCompletionTool>,
    #[allow(dead_code)]
    history: VecDeque<ChatCompletionRequestMessage>,
    /// Background task that holds a long‑lived MCP client connection open (keepalive)
    keepalive: Option<tokio::task::JoinHandle<()>>,
}

impl OpenAiMcpSession {
    /// Initialize a new OpenAiMcpSession (lightweight). Keeps prior signature name `connect` so
    /// existing code (spawn_openai_connect) continues to work, but it no longer establishes any
    /// MCP peer socket – it only prepares an OpenAI client and optional system prompt history.
    ///
    /// addr: retained for compatibility / logging (not currently used for a network dial here)
    /// model: OpenAI model identifier
    /// system_prompt: optional system prompt inserted as first system message in history
    pub async fn connect(addr: &str, model: String, system_prompt: Option<String>) -> Result<Self> {
        // Acquire API key from env; empty key will produce auth errors later (surface via OpenAI client calls)
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        if api_key.is_empty() { log::warn!("OPENAI_API_KEY not set – OpenAI calls will fail until provided"); }

        let config = OpenAIConfig::new().with_api_key(api_key);
        let oa_client = OpenAIClient::with_config(config);

        let mut history: VecDeque<ChatCompletionRequestMessage> = VecDeque::new();
        if let Some(sp) = system_prompt {
            history.push_back(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(sp)
                    .build()? // propagate any build errors
                    .into(),
            );
        }

        log::info!("Initialized OpenAiMcpSession (addr='{}', model='{}')", addr, model);
        Ok(Self {
            oa_client,
            model,
            mcp_addr: addr.to_string(),
            openai_tools: Vec::new(),
            history,
            keepalive: None,
        })
    }
    /// Stream (single-shot currently) command completions using Responses API + JSON Schema (no NDJSON, no code fences).
    pub async fn stream_command_completions(
        &self,
        partial: &str,
        shell: &ShellType,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
        progress_tx: CrossbeamSender<crate::mcp::DiagnosticResponse>,
    ) -> Result<()> {
        use schemars::JsonSchema;
        use schemars::schema_for;
        log::info!("stream_command_completions(responses+schema): partial='{}' shell={:?}", partial, shell);

    #[derive(Debug, serde::Serialize, serde::Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Suggestion { completion: String, description: String, category: String, confidence: f32 }
    #[derive(Debug, serde::Serialize, serde::Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Suggestions { suggestions: Vec<Suggestion> }

    let schema_root = schema_for!(Suggestions); // RootSchema
    // Some schemars versions expose RootSchema { schema: SchemaObject, ... }; if not, serialize whole root.
    let schema_value = serde_json::to_value(&schema_root).unwrap_or(serde_json::json!({"type":"object"}));

        // Determine mode: command name completion vs argument (parameter) completion
    let raw = partial;
    let trimmed = raw.trim_end();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    // Detect if cursor is at a new token (ends with space) – avoid pattern API for older compilers
    let cursor_new_token = raw.chars().last().map(|c| c.is_whitespace()).unwrap_or(false);
        enum Mode<'a> { Command { fragment: &'a str }, Argument { command: &'a str, fragment: &'a str } }
        let mode = if parts.is_empty() {
            Mode::Command { fragment: "" }
        } else if parts.len() == 1 && !trimmed.contains(' ') {
            // Only first token being typed
            Mode::Command { fragment: parts[0] }
        } else {
            let command = parts[0];
            let last_token = if cursor_new_token { "" } else { *parts.last().unwrap_or(&"") };
            if last_token.starts_with('-') || (cursor_new_token && raw.ends_with(" -")) {
                // Argument mode
                let frag = if last_token.starts_with('-') { &last_token[1..] } else { "" };
                Mode::Argument { command, fragment: frag }
            } else if parts.len()==1 { // fallback
                Mode::Command { fragment: parts[0] }
            } else {
                // After command but not starting dash yet – treat as argument mode awaiting dash suggestions
                Mode::Argument { command, fragment: last_token.strip_prefix('-').unwrap_or(last_token) }
            }
        };

        // Compose prompt tailored to mode.
        let base_schema_instr = "Return ONLY a JSON object matching the provided JSON Schema with key 'suggestions' (2-5 items).";
        let prompt = match mode {
            Mode::Command { fragment } => format!(
                "{base_schema_instr}\nTask: Suggest 2-5 full PowerShell command names (Verb-Noun) starting with fragment '{{fragment}}' (case-insensitive).\nRules:\n- Provide canonical cased command names only (no arguments).\n- No duplicates.\n- Each suggestion object: completion=command name, description=short purpose, category: process|service|system|filesystem|network|security|logs|package|other, confidence 0-1.\nFragment: '{fragment}'."
            ),
            Mode::Argument { command, fragment } => format!(
                "{base_schema_instr}\nContext: User is adding parameters to PowerShell command '{command}'. Partial parameter fragment: '{fragment}'.\nTask: Suggest 2-5 parameter names (with leading '-') appropriate for '{command}' that start with fragment if fragment not empty; otherwise common/important parameters.\nRules:\n- Only parameter switches (e.g., -ErrorAction).\n- Do NOT repeat parameters already present earlier in the line.\n- Keep original PowerShell casing.\n- Each suggestion object: completion=parameter (with leading dash), description concise, category reflect domain (e.g., system, logs, security, other), confidence 0-1."
            ),
        };

        let text_cfg = TextConfig { 
            format: TextResponseFormat::JsonSchema(ResponseFormatJsonSchema {
                name: "command_completions".into(),
                description: Some("Structured command completions".into()),
                schema: Some(schema_value),
                strict: Some(true),
            }),
            verbosity: None
        };

        // Build request body (Responses API) with streaming enabled
        let request_body = CreateResponse {
            input: Input::Text(prompt),
            model: self.model.clone(),
            stream: Some(true),
            text: Some(text_cfg),
            ..Default::default()
        };

        // We'll manually call /responses with stream=true and parse SSE events to avoid the non-streaming create() deserialization error.
        #[derive(Debug, serde::Deserialize)]
        struct SseEventMinimal {
            #[serde(rename = "type")] kind: Option<String>,
            delta: Option<String>,
            text: Option<String>,
        }

        let api_base = std::env::var("OPENAI_API_BASE").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let url = format!("{}/responses", api_base.trim_end_matches('/'));
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        if api_key.is_empty() { log::warn!("OPENAI_API_KEY not set – streaming will fail"); }

        let http = reqwest::Client::new();
        let body_bytes = serde_json::to_vec(&request_body)?;
        let send_fut = http
            .post(&url)
            .bearer_auth(api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .body(body_bytes)
            .send();

    let resp = tokio::select! {
            r = send_fut => r?,
            _ = &mut cancel_rx => { log::info!("responses streaming cancelled before start for '{}'", partial); return Err(anyhow::anyhow!("cancelled")); }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            log::error!("Responses streaming HTTP error {} body={} partial='{}'", status, err_text, partial);
            return Ok(());
        }

    let mut sse = resp.bytes_stream();
    let mut json_buffer = String::new();
    let mut emitted = false;              // have we emitted at least one batch (for cancellation semantics)
    let mut last_emitted_count: usize = 0; // cumulative suggestions emitted so far

        // Attempt to parse any fully completed suggestion objects from the in‑progress JSON buffer.
        // Returns count emitted (cumulative) or previous count if nothing new.
        fn try_emit_partial(
            session: &OpenAiMcpSession,
            buf: &str,
            last_count: &mut usize,
            progress_tx: &CrossbeamSender<crate::mcp::DiagnosticResponse>,
            emitted_flag: &mut bool,
        ) {
            // Fast path: need the suggestions key and an opening '['
            if !buf.contains("\"suggestions\"") { return; }
            let key_pos = match buf.find("\"suggestions\"") { Some(p) => p, None => return };
            let slice = &buf[key_pos..];
            let arr_start = match slice.find('[') { Some(i) => key_pos + i, None => return };
            // Scan forward collecting complete top-level objects inside the suggestions array.
            let mut i = arr_start + 1; // position after '['
            let mut depth = 0i32; // object brace depth within current suggestion object
            let mut objs: Vec<&str> = Vec::new();
            let bytes = buf.as_bytes();
            let mut obj_start: Option<usize> = None;
            let mut in_str = false;
            let mut escape = false;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if in_str {
                    if escape { escape = false; }
                    else if c == '\\' { escape = true; }
                    else if c == '"' { in_str = false; }
                    i += 1; continue;
                } else {
                    match c {
                        '"' => { in_str = true; },
                        '{' => {
                            if depth == 0 { obj_start = Some(i); }
                            depth += 1;
                        },
                        '}' => {
                            depth -= 1;
                            if depth == 0 { if let Some(s) = obj_start { objs.push(&buf[s..=i]); obj_start=None; } }
                            if depth < 0 { break; }
                        },
                        ']' => { break; },
                        _ => {}
                    }
                }
                i += 1;
            }
            if objs.is_empty() { return; }
            if objs.len() <= *last_count { return; }
            // Avoid emitting only a single suggestion unless the array appears finished.
            let array_finished = buf[arr_start..].contains(']');
            if *last_count == 0 && objs.len() < 2 && !array_finished { return; }
            // Build minimal JSON with completed objects only
            let joined = objs.join(",");
            let candidate = format!("{{\"suggestions\":[{}]}}", joined);
            // Try to deserialize; ignore errors (probably partial last object)
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&candidate) {
                if parsed.get("suggestions").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0) > *last_count {
                    if session.process_suggestions_json(&candidate, progress_tx.clone()).is_ok() {
                        *last_count = parsed["suggestions"].as_array().unwrap().len();
                        *emitted_flag = true; // we've provided UI data
                    }
                }
            }
        }

    while let Some(chunk_res) = tokio::select! { c = sse.next() => c, _ = &mut cancel_rx => { if emitted { log::info!("responses streaming cancelled after emission for '{}'", partial); return Ok(()); } else { log::info!("responses streaming cancelled mid-stream for '{}'", partial); return Err(anyhow::anyhow!("cancelled")); } } } {
            let chunk = match chunk_res { Ok(c) => c, Err(e) => { log::error!("SSE network chunk error: {e}"); break; } };
            let text = match std::str::from_utf8(&chunk) { Ok(t) => t, Err(_) => continue };
            for raw_line in text.split('\n') {
                let line = raw_line.trim();
                if line.is_empty() { continue; }
                if !line.starts_with("data:") { continue; }
                let payload = line.strip_prefix("data:").unwrap().trim();
                if payload.is_empty() { continue; }
                if payload == "[DONE]" { break; }
                match serde_json::from_str::<SseEventMinimal>(payload) {
                    Ok(evt) => {
                        match evt.kind.as_deref() {
                            Some("response.output_text.delta") => { if let Some(d) = evt.delta { json_buffer.push_str(&d); try_emit_partial(self, &json_buffer, &mut last_emitted_count, &progress_tx, &mut emitted); } },
                            Some("response.output_text.done") => { if let Some(full) = evt.text { json_buffer = full; } },
                            Some("response.completed") => {
                                if !emitted { if self.process_suggestions_json(&json_buffer, progress_tx.clone()).is_ok() { emitted = true; } }
                            }
                            _ => { /* ignore other event types */ }
                        }
                        // Try early parse on plausible JSON end to give UI faster updates
                        if !emitted && json_buffer.contains("\"suggestions\"") && json_buffer.trim_end().ends_with('}') {
                            if self.process_suggestions_json(&json_buffer, progress_tx.clone()).is_ok() { emitted = true; }
                        }
                    }
                    Err(e) => {
                        // benign for partial chunks
                        log::trace!("Unparsed SSE data fragment: {} (err={})", payload, e);
                    }
                }
            }
            if emitted && last_emitted_count >= 2 { break; } // stop early if we already gave reasonable batch
        }
    if !emitted && !json_buffer.is_empty() {
        let _ = self.process_suggestions_json(&json_buffer, progress_tx);
        }
        Ok(())
    }

    fn process_suggestions_json(&self, raw: &str, progress_tx: CrossbeamSender<crate::mcp::DiagnosticResponse>) -> Result<()> {
        #[derive(Debug, serde::Deserialize)]
        struct Suggestion { completion: String, description: String, category: String, confidence: f32 }
        #[derive(Debug, serde::Deserialize)]
        struct Suggestions { suggestions: Vec<Suggestion> }
        let parsed: Suggestions = serde_json::from_str(raw.trim())?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for s in parsed.suggestions.into_iter().take(5) {
            let comp = s.completion.trim();
            if comp.is_empty() { continue; }
            if !seen.insert(comp.to_lowercase()) { continue; }
            out.push(crate::mcp::CommandCompletion { completion: comp.to_string(), description: Some(s.description), category: Some(s.category), confidence: s.confidence.clamp(0.0,1.0) });
        }
        if out.is_empty() { return Ok(()); }
        let _ = progress_tx.try_send(crate::mcp::DiagnosticResponse::CommandCompletions { completions: out, context_info: None });
        log::info!("emitted streaming suggestions ({} chars raw)", raw.len());
        Ok(())
    }

    /// Request command completions via MCP tool using the persistent peer (no additional sockets)
    pub async fn request_command_completions(&self, _partial: &str, _shell: &ShellType, _context: Option<&str>) -> Result<Vec<crate::mcp::CommandCompletion>> { Ok(Vec::new()) }

    /// AI-driven command completion using natural conversation. 
    /// The model decides whether to call MCP tools (like complete_command) or respond directly.
    /// This leverages the MCP server properly instead of bypassing it.
    pub async fn ai_command_completions(&mut self, _partial: &str, _shell: &ShellType) -> Result<Vec<crate::mcp::CommandCompletion>> { Ok(Vec::new()) }

    /// Extract CommandCompletion results from recent MCP tool call responses in history
    #[allow(dead_code)]
    fn extract_completions_from_history(&self) -> Result<Vec<crate::mcp::CommandCompletion>> { Ok(Vec::new()) }
}

impl Drop for OpenAiMcpSession {
    fn drop(&mut self) {
        if let Some(handle) = self.keepalive.take() {
            handle.abort();
        }
    }
}

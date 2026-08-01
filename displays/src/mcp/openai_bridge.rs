#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]

use anyhow::Result;
use serde_json;
use std::collections::{VecDeque, HashSet};
use crossbeam::channel::Sender as CrossbeamSender;

use crate::{ai::{effective_api_base, effective_api_key, effective_model}, mcp::mcp::ShellType, openai::{
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs,
        ChatCompletionTool,
    },
    Client as OpenAIClient,
}};
use futures::StreamExt;

/// A bridge session that connects Gemini Chat Completions to an MCP server over TCP.
pub struct OpenAiMcpSession {
    pub oa_client: OpenAIClient<OpenAIConfig>,
    pub model: String,
    #[allow(dead_code)]
    mcp_addr: String,
    #[allow(dead_code)]
    openai_tools: Vec<ChatCompletionTool>,
    #[allow(dead_code)]
    history: VecDeque<ChatCompletionRequestMessage>,
    keepalive: Option<tokio::task::JoinHandle<()>>,
}

impl OpenAiMcpSession {
    pub async fn connect(addr: &str, model: String, system_prompt: Option<String>) -> Result<Self> {
        let api_key = effective_api_key();
        if api_key.is_empty() { log::warn!("No OpenAI/Gemini API key set – completions will fail until provided"); }

        let api_base = effective_api_base();
        let model = effective_model(&model);

        let config = OpenAIConfig::new()
            .with_api_key(&api_key)
            .with_api_base(&api_base);
        let oa_client = OpenAIClient::with_config(config);

        let mut history: VecDeque<ChatCompletionRequestMessage> = VecDeque::new();
        if let Some(sp) = system_prompt {
            history.push_back(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(sp)
                    .build()?
                    .into(),
            );
        }

        log::info!("Initialized GeminiMcpSession (addr='{}', model='{}')", addr, model);
        Ok(Self {
            oa_client,
            model,
            mcp_addr: addr.to_string(),
            openai_tools: Vec::new(),
            history,
            keepalive: None,
        })
    }

    /// Stream command completions from the Responses API, emitting suggestions as
    /// soon as each object in the model's JSON array closes.
    pub async fn stream_command_completions(
        &self,
        partial: &str,
        shell: &ShellType,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
        progress_tx: CrossbeamSender<crate::mcp::DiagnosticResponse>,
    ) -> Result<()> {
        use schemars::JsonSchema;
        use schemars::schema_for;
        log::info!("stream_command_completions(chat+json): partial='{}' shell={:?}", partial, shell);

        #[derive(Debug, serde::Serialize, serde::Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Suggestion { completion: String, description: String, category: String, confidence: f32 }
        #[derive(Debug, serde::Serialize, serde::Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Suggestions { suggestions: Vec<Suggestion> }

        let schema_root = schema_for!(Suggestions);
        let schema_value = serde_json::to_value(&schema_root).unwrap_or(serde_json::json!({"type":"object"}));

        let raw = partial;
        let trimmed = raw.trim_end();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cursor_new_token = raw.chars().last().map(|c| c.is_whitespace()).unwrap_or(false);
        enum Mode<'a> { Command { fragment: &'a str }, Argument { command: &'a str, fragment: &'a str } }
        let mode = if parts.is_empty() {
            Mode::Command { fragment: "" }
        } else if parts.len() == 1 && !trimmed.contains(' ') {
            Mode::Command { fragment: parts[0] }
        } else {
            let command = parts[0];
            let last_token = if cursor_new_token { "" } else { *parts.last().unwrap_or(&"") };
            if last_token.starts_with('-') || (cursor_new_token && raw.ends_with(" -")) {
                let frag = if last_token.starts_with('-') { &last_token[1..] } else { "" };
                Mode::Argument { command, fragment: frag }
            } else if parts.len() == 1 {
                Mode::Command { fragment: parts[0] }
            } else {
                Mode::Argument { command, fragment: last_token.strip_prefix('-').unwrap_or(last_token) }
            }
        };

        let base_schema_instr = format!(
            "Return ONLY a JSON object matching this schema: {}\nKey 'suggestions' must contain 2-5 items.",
            serde_json::to_string(&schema_value).unwrap_or_default()
        );
        let prompt = match mode {
            Mode::Command { fragment } => format!(
                "{base_schema_instr}\nTask: Suggest 2-5 full PowerShell command names (Verb-Noun) starting with fragment (case-insensitive).\nRules:\n- Provide canonical cased command names only (no arguments).\n- No duplicates.\n- Each suggestion object: completion=command name, description=short purpose, category: process|service|system|filesystem|network|security|logs|package|other, confidence 0-1.\nFragment: '{fragment}'."
            ),
            Mode::Argument { command, fragment } => format!(
                "{base_schema_instr}\nContext: User is adding parameters to PowerShell command '{command}'. Partial parameter fragment: '{fragment}'.\nTask: Suggest 2-5 parameter names (with leading '-') appropriate for '{command}' that start with fragment if fragment not empty; otherwise common/important parameters.\nRules:\n- Only parameter switches (e.g., -ErrorAction).\n- Do NOT repeat parameters already present earlier in the line.\n- Keep original PowerShell casing.\n- Each suggestion object: completion=parameter (with leading dash), description concise, category reflect domain, confidence 0-1."
            ),
        };

        let api_base = effective_api_base();
        let url = format!("{}/responses", api_base.trim_end_matches('/'));
        let api_key = effective_api_key();
        if api_key.is_empty() { log::warn!("No OpenAI/Gemini API key set – streaming will fail"); }

        let request_body = serde_json::json!({
            "model": self.model,
            "input": prompt,
            "stream": true,
            "temperature": 0.2,
            "response_mime_type": "application/json",
        });

        let http = reqwest::Client::new();
        let body_bytes = serde_json::to_vec(&request_body)?;
        let send_fut = http
            .post(&url)
            .bearer_auth(&api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .body(body_bytes)
            .send();

        let resp = tokio::select! {
            r = send_fut => r?,
            _ = &mut cancel_rx => { log::info!("streaming cancelled before start for '{}'", partial); return Err(anyhow::anyhow!("cancelled")); }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            log::error!("Chat completions streaming HTTP error {} body={} partial='{}'", status, err_text, partial);
            return Ok(());
        }

        let mut sse = resp.bytes_stream();
        let mut json_buffer = String::new();
        let mut emitted = false;
        let mut last_emitted_count: usize = 0;

        fn try_emit_partial(
            session: &OpenAiMcpSession,
            buf: &str,
            last_count: &mut usize,
            progress_tx: &CrossbeamSender<crate::mcp::DiagnosticResponse>,
            emitted_flag: &mut bool,
        ) {
            if !buf.contains("\"suggestions\"") { return; }
            let key_pos = match buf.find("\"suggestions\"") { Some(p) => p, None => return };
            let slice = &buf[key_pos..];
            let arr_start = match slice.find('[') { Some(i) => key_pos + i, None => return };
            let mut i = arr_start + 1;
            let mut depth = 0i32;
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
                            if depth == 0 { if let Some(s) = obj_start { objs.push(&buf[s..=i]); obj_start = None; } }
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
            let array_finished = buf[arr_start..].contains(']');
            if *last_count == 0 && objs.len() < 2 && !array_finished { return; }
            let joined = objs.join(",");
            let candidate = format!("{{\"suggestions\":[{}]}}", joined);
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&candidate) {
                if parsed.get("suggestions").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0) > *last_count {
                    if session.process_suggestions_json(&candidate, progress_tx.clone()).is_ok() {
                        *last_count = parsed["suggestions"].as_array().unwrap().len();
                        *emitted_flag = true;
                    }
                }
            }
        }

        while let Some(chunk_res) = tokio::select! {
            c = sse.next() => c,
            _ = &mut cancel_rx => {
                if emitted { log::info!("streaming cancelled after emission for '{}'", partial); return Ok(()); }
                else { log::info!("streaming cancelled mid-stream for '{}'", partial); return Err(anyhow::anyhow!("cancelled")); }
            }
        } {
            let chunk = match chunk_res { Ok(c) => c, Err(e) => { log::error!("SSE network chunk error: {e}"); break; } };
            let text = match std::str::from_utf8(&chunk) { Ok(t) => t, Err(_) => continue };
            for raw_line in text.split('\n') {
                let line = raw_line.trim();
                if line.is_empty() { continue; }
                if !line.starts_with("data:") { continue; }
                let payload = line.strip_prefix("data:").unwrap().trim();
                if payload.is_empty() { continue; }
                if payload == "[DONE]" { break; }
                match serde_json::from_str::<serde_json::Value>(payload) {
                    Ok(event) => {
                        match event["type"].as_str() {
                            // Both documented spellings of the text delta event.
                            Some("response.output_text.delta") | Some("response.content_part.delta") => {
                                let delta = event["delta"].as_str().or_else(|| event["delta"]["text"].as_str());
                                if let Some(content) = delta {
                                    json_buffer.push_str(content);
                                    try_emit_partial(self, &json_buffer, &mut last_emitted_count, &progress_tx, &mut emitted);
                                }
                            }
                            Some("response.completed") | Some("response.done") | Some("response.failed")
                            | Some("response.incomplete") => {
                                if !emitted {
                                    let _ = self.process_suggestions_json(&json_buffer, progress_tx.clone());
                                    emitted = true;
                                }
                            }
                            _ => {}
                        }
                        if !emitted && json_buffer.contains("\"suggestions\"") && json_buffer.trim_end().ends_with('}') {
                            if self.process_suggestions_json(&json_buffer, progress_tx.clone()).is_ok() { emitted = true; }
                        }
                    }
                    Err(e) => {
                        log::trace!("Unparsed SSE data fragment: {} (err={})", payload, e);
                    }
                }
            }
            if emitted && last_emitted_count >= 2 { break; }
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

    pub async fn request_command_completions(&self, _partial: &str, _shell: &ShellType, _context: Option<&str>) -> Result<Vec<crate::mcp::CommandCompletion>> { Ok(Vec::new()) }

    pub async fn ai_command_completions(&mut self, _partial: &str, _shell: &ShellType) -> Result<Vec<crate::mcp::CommandCompletion>> { Ok(Vec::new()) }

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

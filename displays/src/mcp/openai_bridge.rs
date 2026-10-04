#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]

use anyhow::Result;
use serde_json;
use std::collections::HashSet;
use crossbeam::channel::Sender as CrossbeamSender;

use crate::{ai::{custom_api_base, effective_api_key, effective_model}, mcp::mcp::ShellType};
use database::schema::ZeroclawGateway;
use futures::StreamExt;
use std::time::Duration;

/// `[[model_routes]]` hint the ZeroClaw gateway resolves to the fast pool.
const ZEROCLAW_HINT: &str = "quick";
/// Longest a ZeroClaw completion may take.
const ZEROCLAW_TIMEOUT: Duration = Duration::from_secs(30);

/// Command suggestions from the ZeroClaw gateway, or from the user's own OpenAI-compatible endpoint.
pub struct OpenAiMcpSession {
    pub model: String,
}

impl OpenAiMcpSession {
    pub async fn connect(addr: &str, model: String) -> Result<Self> {
        if custom_api_base().is_some() && effective_api_key().is_empty() {
            log::warn!("Custom completion endpoint set without an API key");
        }

        let model = effective_model(&model);

        log::debug!("Initialized command suggestion session (addr='{}', model='{}')", addr, model);
        Ok(Self { model })
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
        log::debug!("stream_command_completions(chat+json): partial='{}' shell={:?}", partial, shell);

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

        let Some(api_base) = custom_api_base() else {
            return self.complete_via_zeroclaw(&prompt, partial, cancel_rx, &progress_tx).await;
        };
        let url = format!("{}/responses", api_base.trim_end_matches('/'));
        let api_key = effective_api_key();
        if api_key.is_empty() { log::warn!("No API key set for the custom completion endpoint – streaming will fail"); }

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
            _ = &mut cancel_rx => { log::debug!("streaming cancelled before start for '{}'", partial); return Err(anyhow::anyhow!("cancelled")); }
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
                if emitted { log::debug!("streaming cancelled after emission for '{}'", partial); return Ok(()); }
                else { log::debug!("streaming cancelled mid-stream for '{}'", partial); return Err(anyhow::anyhow!("cancelled")); }
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

    /// Asks the ZeroClaw gateway for the suggestions; quiet when the user has no gateway access.
    async fn complete_via_zeroclaw(
        &self,
        prompt: &str,
        partial: &str,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
        progress_tx: &CrossbeamSender<crate::mcp::DiagnosticResponse>,
    ) -> Result<()> {
        let gateway = match ZeroclawGateway::fetch().await {
            Ok(Some(gateway)) => gateway,
            Ok(None) => {
                log::debug!("command suggestions need ZeroClaw gateway access");
                return Ok(());
            }
            Err(e) => {
                log::warn!("ZeroClaw gateway lookup failed: {e}");
                return Ok(());
            }
        };
        let reply = tokio::select! {
            r = tokio::time::timeout(ZEROCLAW_TIMEOUT, zeroclaw_complete(&gateway, prompt)) => r,
            _ = &mut cancel_rx => return Ok(()),
        };
        let text = match reply {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => {
                log::error!("ZeroClaw completion for '{partial}' failed: {e}");
                return Ok(());
            }
            Err(_) => {
                log::warn!("ZeroClaw completion for '{partial}' took over {}s", ZEROCLAW_TIMEOUT.as_secs());
                return Ok(());
            }
        };
        match json_object(&text).map(|json| self.process_suggestions_json(json, progress_tx.clone())) {
            Some(Ok(())) => {}
            Some(Err(e)) => log::warn!("ZeroClaw suggestions for '{partial}' did not parse: {e}"),
            None => log::warn!("ZeroClaw completion for '{partial}' held no JSON object"),
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
        log::debug!("emitted streaming suggestions ({} chars raw)", raw.len());
        Ok(())
    }
}

/// One completion from the gateway's `POST /api/complete`.
async fn zeroclaw_complete(gateway: &ZeroclawGateway, prompt: &str) -> Result<String> {
    let url = format!("{}/api/complete", gateway.url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(&gateway.token)
        .json(&serde_json::json!({ "hint": ZEROCLAW_HINT, "prompt": prompt, "temperature": 0.2 }))
        .send()
        .await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        let why = body.get("error").and_then(|v| v.as_str()).unwrap_or("no error message");
        anyhow::bail!("HTTP {} from /api/complete: {why}", status.as_u16());
    }
    body.get("response")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("/api/complete returned no response"))
}

/// The outermost `{…}` of a reply that may wrap its JSON in prose or a code fence.
fn json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (start < end).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn json_object_strips_prose_and_fences() {
        assert_eq!(json_object("```json\n{\"a\":1}\n```"), Some("{\"a\":1}"));
        assert_eq!(json_object("Here: {\"a\":{\"b\":2}} done"), Some("{\"a\":{\"b\":2}}"));
        assert_eq!(json_object("no json"), None);
        assert_eq!(json_object("} {"), None);
    }

    /// Serves one canned HTTP response and hands back the request it received.
    async fn one_shot_server(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let served = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text[..head_end]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length { break; }
                }
                if n == 0 { break; }
            }
            socket.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).to_string()
        });
        (url, served)
    }

    #[tokio::test]
    async fn zeroclaw_complete_posts_the_hint_with_the_token() {
        let (url, served) = one_shot_server("200 OK", r#"{"response":"{\"suggestions\":[]}","model":"zc-quick","hint":"quick"}"#).await;
        let gateway = ZeroclawGateway { url, token: "zc_test".into() };
        let reply = zeroclaw_complete(&gateway, "Get-Pro").await.unwrap();
        assert_eq!(reply, r#"{"suggestions":[]}"#);
        let request = served.await.unwrap();
        assert!(request.starts_with("POST /api/complete "), "{request}");
        assert!(request.to_ascii_lowercase().contains("authorization: bearer zc_test"), "{request}");
        assert!(request.contains(r#""hint":"quick""#) && request.contains("Get-Pro"), "{request}");
    }

    #[tokio::test]
    async fn zeroclaw_complete_reports_the_gateway_error() {
        let (url, _served) = one_shot_server("400 Bad Request", r#"{"error":"no [[model_routes]] entry has hint `quick`"}"#).await;
        let gateway = ZeroclawGateway { url, token: "t".into() };
        let err = zeroclaw_complete(&gateway, "x").await.unwrap_err().to_string();
        assert!(err.contains("HTTP 400") && err.contains("hint `quick`"), "{err}");
    }
}

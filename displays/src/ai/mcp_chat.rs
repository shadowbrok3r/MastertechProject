#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
//! Streaming chat that exposes the full Mastertech MCP toolset to an
//! OpenAI-compatible model. Connects an rmcp client to the plugin MCP server
//! on TCP :9003, advertises its tools to the model, and dispatches the model's
//! tool calls back through that client until it produces a final answer.

use std::collections::HashMap;

use anyhow::Result;
use crossbeam::channel::Sender;
use futures::StreamExt;
use rmcp::model::CallToolRequestParams;

use crate::ai::{effective_api_base, effective_api_key, effective_model, gpts};
use crate::tabs::ai_playground::{ChatMessage, ChatMessageType, SentFrom};
use crate::ui_tools::icons;

const PLUGIN_MCP_ADDR: &str = "127.0.0.1:9003";

const SYSTEM_PROMPT: &str = "You are the Mastertech assistant embedded in the technician app. \
You can call the provided Mastertech tools (SurrealDB queries, customer/order/computer lookups, \
diagnostics, plugin and remote-UI control) to answer questions and act on the user's behalf. \
Prefer a tool over guessing when a question concerns live data. Keep answers concise. \
When the user refers to \"this computer\"/\"this PC\" without naming a client, they mean the local \
machine this app is running on — inspect it with the local system and diagnostic tools.";

fn send(tx: &Sender<ChatMessage>, thread_id: &str, id: String, from: SentFrom, content: ChatMessageType) {
    let _ = tx.try_send(ChatMessage { id, thread_id: thread_id.to_string(), ts: crate::tabs::ai_playground::now_ts(), from, content });
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Per-machine override of the compiled-in gateway.
pub fn zeroclaw_config_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    std::path::Path::new(&base).join("MasterTech").join("zeroclaw.json")
}

fn zeroclaw_file() -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(zeroclaw_config_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Trims a gateway pair, returning `None` when either half is blank.
fn normalize_gateway(url: &str, token: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/').to_string();
    let token = token.trim().to_string();
    (!url.is_empty() && !token.is_empty()).then_some((url, token))
}

/// ZeroClaw gateway target. The URL resolves from the `ZEROCLAW_GATEWAY_URL`
/// environment, then `<APPDATA|HOME>/MasterTech/zeroclaw.json`, then the value
/// compiled in from the repo-root `.env`. The bearer token is never compiled in
/// and comes from the environment or that same file.
pub fn zeroclaw_gateway() -> Option<(String, String)> {
    let file = zeroclaw_file();
    let from_file = |key: &str| {
        file.as_ref()
            .and_then(|v| v[key].as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let from_env = |key: &str| {
        std::env::var(key)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let url = from_env("ZEROCLAW_GATEWAY_URL")
        .or_else(|| from_file("url"))
        .unwrap_or_else(|| database::ZEROCLAW_GATEWAY_URL.to_string());
    let token = from_env("ZEROCLAW_GATEWAY_TOKEN").or_else(|| from_file("token"))?;
    normalize_gateway(&url, &token)
}

/// Hostnames the BSOD autopilot needs a standing admin session to, from
/// `autopilot_hosts` in the gateway config file. Remote MCP tools fail without
/// a live session, so the console must hold one open for each.
pub fn autopilot_hosts() -> Vec<String> {
    // ensure_sessions runs every frame; re-read at most every 30s.
    static CACHE: std::sync::OnceLock<std::sync::Mutex<(std::time::Instant, Vec<String>)>> =
        std::sync::OnceLock::new();
    let cell = CACHE.get_or_init(|| {
        std::sync::Mutex::new((
            std::time::Instant::now() - std::time::Duration::from_secs(3600),
            Vec::new(),
        ))
    });
    if let Ok(mut guard) = cell.lock() {
        if guard.0.elapsed() < std::time::Duration::from_secs(30) {
            return guard.1.clone();
        }
        let fresh = read_autopilot_hosts();
        *guard = (std::time::Instant::now(), fresh.clone());
        return fresh;
    }
    read_autopilot_hosts()
}

fn read_autopilot_hosts() -> Vec<String> {
    std::fs::read_to_string(zeroclaw_config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v["autopilot_hosts"].as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Agent alias for dispatched turns: env, then config file, then default.
pub fn zeroclaw_agent() -> String {
    if let Ok(a) = std::env::var("ZEROCLAW_AGENT") {
        if !a.trim().is_empty() {
            return a.trim().to_string();
        }
    }
    std::fs::read_to_string(zeroclaw_config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v["agent"].as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "diagnostician".to_string())
}

/// One diagnose turn through the ZeroClaw dispatcher: POST /webhook?agent=<alias>,
/// then emit the agent's final reply. `ZEROCLAW_AGENT` overrides the target alias.
pub async fn zeroclaw_diagnose(prompt: String, thread_id: String, response_tx: Sender<ChatMessage>) {
    let Some((url, token)) = zeroclaw_gateway() else {
        send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Error(
            "ZeroClaw gateway not configured — set ZEROCLAW_GATEWAY_TOKEN in the environment, \
             or write the url and token into MasterTech/zeroclaw.json."
                .into(),
        ));
        send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Done);
        return;
    };
    let agent = zeroclaw_agent();
    send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Text(format!(
        "{}dispatched to ZeroClaw agent `{agent}` — it gathers context, delegates deep analysis \
         to Claude Code, and replies here when done…",
        crate::ai::claude_code::TOOL_PREFIX
    )));

    let turn = async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(900))
            .build()?;
        let resp = client
            .post(format!("{url}/webhook"))
            .query(&[("agent", agent.as_str())])
            .bearer_auth(&token)
            .header("X-Idempotency-Key", new_id())
            .json(&serde_json::json!({ "message": prompt }))
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Ok::<_, reqwest::Error>((status, body))
    };

    match turn.await {
        Ok((status, body)) if status.is_success() => {
            let text = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["response"].as_str().map(str::to_string))
                .unwrap_or(body);
            send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Text(text));
        }
        Ok((status, body)) => {
            let snippet: String = body.chars().take(300).collect();
            send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Error(format!(
                "ZeroClaw gateway {status}: {snippet}"
            )));
        }
        Err(e) => {
            let msg = if e.is_timeout() {
                "ZeroClaw turn exceeded 900s — check the daemon and gateway request timeout.".to_string()
            } else {
                format!("ZeroClaw gateway unreachable: {e}")
            };
            send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Error(msg));
        }
    }
    send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Done);
}

/// Text of a `*.delta` event, which carries either a bare string or a content part.
fn delta_text(event: &serde_json::Value) -> Option<&str> {
    event["delta"].as_str().or_else(|| event["delta"]["text"].as_str())
}

fn text_of(result: &rmcp::model::CallToolResult) -> String {
    let body: String = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    if body.is_empty() {
        serde_json::to_string(&result.structured_content).unwrap_or_else(|_| "(no content)".to_string())
    } else {
        body
    }
}

async fn connect_and_list_tools() -> Result<(
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    Vec<rmcp::model::Tool>,
)> {
    let stream = tokio::net::TcpStream::connect(PLUGIN_MCP_ADDR).await?;
    let (read, write) = tokio::io::split(stream);
    let client = rmcp::serve_client((), (read, write)).await?;
    let tools = client.list_all_tools().await?;
    Ok((client, tools))
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args_json: &str,
) -> String {
    let arguments = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(args_json).ok();
    let mut params = CallToolRequestParams::new(name.to_string());
    if let Some(map) = arguments {
        params = params.with_arguments(map);
    }
    match client.call_tool(params).await {
        Ok(result) => text_of(&result),
        Err(e) => format!("tool '{name}' failed: {e}"),
    }
}

/// Builds OpenAI-format `{role, content}` message objects from prior thread
/// messages (Me -> user, Gpt text -> assistant). Reasoning, tool-activity, and
/// non-text messages are skipped. The new user input is appended by `stream_chat`.
pub fn history_json_from_messages(msgs: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for m in msgs {
        let text = match &m.content {
            ChatMessageType::Text(t) | ChatMessageType::Code(t) => t.clone(),
            _ => continue,
        };
        if text.trim().is_empty()
            || text.starts_with(icons::WRENCH)
            || text.starts_with(crate::ai::claude_code::TOOL_PREFIX)
        {
            continue;
        }
        let role = match m.from {
            SentFrom::Me => "user",
            SentFrom::Assistant => "assistant",
        };
        out.push(serde_json::json!({ "role": role, "content": text }));
    }
    out
}

/// Streams a model response from the OpenAI-compatible endpoint in `mcp_settings`
/// via raw SSE against the Responses API (so the model's `reasoning` tokens are
/// captured — async-openai drops them). Optionally exposes the Mastertech MCP tools
/// and runs the tool-call loop. Emits `Reasoning`, `Text`, and tool-activity
/// messages over `response_tx`.
///
/// The endpoint is stateless: `store` and `previous_response_id` are rejected, so
/// the whole input list is resent on each turn of the tool loop.
pub async fn stream_chat(
    input: String,
    prior: Vec<serde_json::Value>,
    thread_id: String,
    use_tools: bool,
    response_tx: Sender<ChatMessage>,
) -> Result<()> {
    let base = effective_api_base();
    let key = effective_api_key();
    let model = effective_model(gpts::MODEL);
    let url = format!("{}/responses", base.trim_end_matches('/'));

    if key.trim().is_empty() {
        send(
            &response_tx,
            &thread_id,
            new_id(),
            SentFrom::Assistant,
            ChatMessageType::Error(
                "No API key configured. Add one in Account Settings -> MCP / AI Endpoint and save.".to_string(),
            ),
        );
        send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Done);
        return Ok(());
    }

    let (mcp_client, tools_json) = if use_tools {
        match connect_and_list_tools().await {
            Ok((client, tools)) => {
                let arr = tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "type": "function",
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.schema_as_json_value(),
                        })
                    })
                    .collect::<Vec<_>>();
                (Some(client), arr)
            }
            Err(e) => {
                log::warn!("stream_chat: tools unavailable ({e}); continuing without tools");
                send(
                    &response_tx,
                    &thread_id,
                    new_id(),
                    SentFrom::Assistant,
                    ChatMessageType::Error(format!("Mastertech tools unavailable: {e}")),
                );
                (None, Vec::new())
            }
        }
    } else {
        (None, Vec::new())
    };

    // `prior` is already `{role, content}`, which the Responses API accepts as an
    // input item alongside the `function_call`/`function_call_output` items below.
    let mut items: Vec<serde_json::Value> = Vec::new();
    items.extend(prior);
    items.push(serde_json::json!({ "role": "user", "content": input }));

    let http = reqwest::Client::new();

    loop {
        let mut body = serde_json::json!({
            "model": model,
            "instructions": SYSTEM_PROMPT,
            "input": items,
            "stream": true,
            "reasoning": { "enabled": true },
        });
        if !tools_json.is_empty() {
            body["tools"] = serde_json::Value::Array(tools_json.clone());
            body["tool_choice"] = serde_json::json!("auto");
        }

        let resp = http
            .post(&url)
            .bearer_auth(&key)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Error(format!("Request failed: {e}")));
                send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Done);
                break;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Error(format!("HTTP {status}: {detail}")));
            send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Done);
            break;
        }

        let assistant_id = new_id();
        let think_id = format!("{assistant_id}-think");
        // index -> (call id, name, accumulated args)
        let mut tool_acc: HashMap<u64, (String, String, String)> = HashMap::new();
        let mut buf = String::new();
        let mut sse = resp.bytes_stream();

        while let Some(item) = sse.next().await {
            let bytes = match item {
                Ok(b) => b,
                Err(e) => {
                    log::error!("stream_chat: SSE error: {e}");
                    break;
                }
            };
            buf.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim().to_string();
                buf.drain(..=nl);
                if line.is_empty() || !line.starts_with("data:") {
                    continue;
                }
                let payload = line["data:".len()..].trim();
                if payload == "[DONE]" {
                    continue;
                }
                let json: serde_json::Value = match serde_json::from_str(payload) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                let Some(kind) = json["type"].as_str() else { continue };
                let idx = json["output_index"].as_u64().unwrap_or(0);

                match kind {
                    // Text and reasoning each have two documented event spellings.
                    "response.output_text.delta" | "response.content_part.delta" => {
                        if let Some(text) = delta_text(&json) {
                            if !text.is_empty() {
                                send(&response_tx, &thread_id, assistant_id.clone(), SentFrom::Assistant, ChatMessageType::Text(text.to_string()));
                            }
                        }
                    }
                    "response.reasoning.delta"
                    | "response.reasoning_text.delta"
                    | "response.reasoning_summary_text.delta" => {
                        if let Some(text) = delta_text(&json) {
                            if !text.is_empty() {
                                send(&response_tx, &thread_id, think_id.clone(), SentFrom::Assistant, ChatMessageType::Reasoning(text.to_string()));
                            }
                        }
                    }
                    // Carries call_id and name on `added`, complete arguments on `done`.
                    "response.output_item.added" | "response.output_item.done" => {
                        let item = &json["item"];
                        if item["type"].as_str() == Some("function_call") {
                            let entry = tool_acc.entry(idx).or_default();
                            if let Some(id) = item["call_id"].as_str() {
                                if !id.is_empty() {
                                    entry.0 = id.to_string();
                                }
                            }
                            if let Some(name) = item["name"].as_str() {
                                if !name.is_empty() {
                                    entry.1 = name.to_string();
                                }
                            }
                            if let Some(args) = item["arguments"].as_str() {
                                if !args.is_empty() {
                                    entry.2 = args.to_string();
                                }
                            }
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        if let Some(d) = json["delta"].as_str() {
                            tool_acc.entry(idx).or_default().2.push_str(d);
                        }
                    }
                    "response.function_call_arguments.done" => {
                        if let Some(args) = json["arguments"].as_str() {
                            tool_acc.entry(idx).or_default().2 = args.to_string();
                        }
                    }
                    "response.failed" | "response.incomplete" | "error" => {
                        let detail = json["response"]["error"]["message"]
                            .as_str()
                            .or_else(|| json["message"].as_str())
                            .unwrap_or("the model reported a failed response");
                        send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Error(detail.to_string()));
                    }
                    _ => {}
                }
            }
        }

        if tool_acc.is_empty() {
            send(&response_tx, &thread_id, new_id(), SentFrom::Assistant, ChatMessageType::Done);
            break;
        }

        let mut calls: Vec<(u64, (String, String, String))> = tool_acc.into_iter().collect();
        calls.sort_by_key(|(idx, _)| *idx);

        // Each call is echoed back before its output so the stateless endpoint sees both.
        for (_, (call_id, name, args)) in &calls {
            items.push(serde_json::json!({
                "type": "function_call",
                "call_id": call_id,
                "name": name,
                "arguments": args,
            }));
        }

        for (_, (call_id, name, args)) in calls {
            // Same shape claude_code.rs emits so one renderer handles both paths.
            let line_id = new_id();
            send(
                &response_tx,
                &thread_id,
                line_id.clone(),
                SentFrom::Assistant,
                ChatMessageType::Text(format!(
                    "{}{}  ({})",
                    crate::ai::claude_code::TOOL_PREFIX,
                    name,
                    args.trim()
                )),
            );
            let result_text = match &mcp_client {
                Some(client) => call_tool(client, &name, &args).await,
                None => "Mastertech tools are not connected.".to_string(),
            };
            let body = result_text.trim();
            let tail = if body.is_empty() {
                " ok".to_string()
            } else {
                format!(" ok\n{}", body.chars().take(16_384).collect::<String>())
            };
            send(&response_tx, &thread_id, line_id, SentFrom::Assistant, ChatMessageType::Text(tail));
            items.push(serde_json::json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": result_text,
            }));
        }
    }

    drop(mcp_client);
    Ok(())
}

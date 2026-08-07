//! Lean streaming chat backend for the terminal-mode AI tab.
//!
//! An OpenAI-compatible SSE chat that exposes qc-app's own MCP tools (raw TCP
//! rmcp on 127.0.0.1:9100) to the model and runs the tool-call loop in-process.
//! Ported from `displays::ai::mcp_chat::stream_chat` — no async-openai, no
//! external `claude` CLI. Emits `ChatMessage`s over a crossbeam channel the TUI
//! drains each frame.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use crossbeam::channel::Sender;
use futures::StreamExt;
use rmcp::model::CallToolRequestParams;

/// qc-app's own MCP server (raw TCP rmcp), spawned on the first app tick.
const QC_MCP_ADDR: &str = "127.0.0.1:9100";

/// Prefix marking a tool-activity line so it can be styled dimly and filtered
/// out of the history sent back to the model.
pub const TOOL_PREFIX: &str = "\u{00BB} "; // "» "

const SYSTEM_PROMPT: &str = "You are the Mastertech QC assistant, running ON the QC bench computer being serviced. \
Use the provided QC tools (hardware telemetry, temperatures, stress tests, benchmarks, reports, run status) to inspect and diagnose THIS machine. \
When the user says \"this computer\"/\"this PC\" they mean the local machine this app runs on. \
Prefer a tool over guessing when a question concerns live hardware data. \
Do NOT start stress tests, benchmarks, or other long or destructive operations unless the user explicitly asks. \
Keep answers concise.";

#[derive(Clone, Debug, PartialEq)]
pub enum SentFrom {
    Me,
    /// Any assistant engine; `Gpt` was the pre-Claude name.
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChatMessageType {
    Text(String),
    Reasoning(String),
    Error(String),
    Done,
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub from: SentFrom,
    pub content: ChatMessageType,
}

fn next_id() -> String {
    static N: AtomicU64 = AtomicU64::new(1);
    format!("m{}", N.fetch_add(1, Ordering::Relaxed))
}

/// Text of a `*.delta` event, which carries either a bare string or a content part.
fn delta_text(event: &serde_json::Value) -> Option<&str> {
    event["delta"].as_str().or_else(|| event["delta"]["text"].as_str())
}

fn send(tx: &Sender<ChatMessage>, id: String, from: SentFrom, content: ChatMessageType) {
    let _ = tx.send(ChatMessage { id, from, content });
}

fn effective_api_base() -> String {
    std::env::var("MASTERTECH_AI_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string())
}

fn effective_api_key() -> String {
    std::env::var("MASTERTECH_AI_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .unwrap_or_default()
}

fn effective_model() -> String {
    std::env::var("MASTERTECH_AI_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "deepseek/deepseek-v4-pro".to_string())
}

/// Builds `{role, content}` history from prior messages (Me -> user, Gpt text
/// -> assistant). Reasoning, tool-activity, and empty lines are skipped.
pub fn history_json_from_messages(msgs: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for m in msgs {
        let text = match &m.content {
            ChatMessageType::Text(t) => t.clone(),
            _ => continue,
        };
        if text.trim().is_empty() || text.starts_with(TOOL_PREFIX) {
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

fn summarize(args_json: &str) -> String {
    let trimmed = args_json.trim();
    if trimmed.chars().count() <= 80 {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(80).collect::<String>())
    }
}

async fn connect_and_list_tools() -> Result<(
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    Vec<rmcp::model::Tool>,
)> {
    let stream = tokio::net::TcpStream::connect(QC_MCP_ADDR).await?;
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

/// Streams a model response from the OpenAI-compatible endpoint via raw SSE against
/// the Responses API (capturing `reasoning` tokens), optionally exposing qc-app's MCP
/// tools and running the tool-call loop. Emits `Reasoning`, `Text`, tool-activity, and
/// a terminal `Done` over `tx`.
///
/// The endpoint is stateless: `store` and `previous_response_id` are rejected, so the
/// whole input list is resent on each turn of the tool loop.
pub async fn stream_chat(
    input: String,
    prior: Vec<serde_json::Value>,
    use_tools: bool,
    tx: Sender<ChatMessage>,
) -> Result<()> {
    let base = effective_api_base();
    let key = effective_api_key();
    let model = effective_model();
    let url = format!("{}/responses", base.trim_end_matches('/'));

    if key.trim().is_empty() {
        send(
            &tx,
            next_id(),
            SentFrom::Assistant,
            ChatMessageType::Error(
                "No API key configured. Set MASTERTECH_AI_API_KEY (and optionally MASTERTECH_AI_BASE / MASTERTECH_AI_MODEL) and relaunch."
                    .to_string(),
            ),
        );
        send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Done);
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
                log::error!("ai_backend: tools unavailable ({e}); continuing without tools");
                send(
                    &tx,
                    next_id(),
                    SentFrom::Assistant,
                    ChatMessageType::Error(format!("QC tools unavailable: {e}")),
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
                send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Error(format!("Request failed: {e}")));
                send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Done);
                break;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Error(format!("HTTP {status}: {detail}")));
            send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Done);
            break;
        }

        let assistant_id = next_id();
        let think_id = format!("{assistant_id}-think");
        // index -> (call id, name, accumulated args)
        let mut tool_acc: HashMap<u64, (String, String, String)> = HashMap::new();
        let mut buf = String::new();
        let mut sse = resp.bytes_stream();

        while let Some(item) = sse.next().await {
            let bytes = match item {
                Ok(b) => b,
                Err(e) => {
                    log::error!("ai_backend: SSE error: {e}");
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
                                send(&tx, assistant_id.clone(), SentFrom::Assistant, ChatMessageType::Text(text.to_string()));
                            }
                        }
                    }
                    "response.reasoning.delta"
                    | "response.reasoning_text.delta"
                    | "response.reasoning_summary_text.delta" => {
                        if let Some(text) = delta_text(&json) {
                            if !text.is_empty() {
                                send(&tx, think_id.clone(), SentFrom::Assistant, ChatMessageType::Reasoning(text.to_string()));
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
                        send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Error(detail.to_string()));
                    }
                    _ => {}
                }
            }
        }

        if tool_acc.is_empty() {
            send(&tx, next_id(), SentFrom::Assistant, ChatMessageType::Done);
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
            send(
                &tx,
                next_id(),
                SentFrom::Assistant,
                ChatMessageType::Text(format!("{TOOL_PREFIX}{name}({})", summarize(&args))),
            );
            let result_text = match &mcp_client {
                Some(client) => call_tool(client, &name, &args).await,
                None => "QC tools are not connected.".to_string(),
            };
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

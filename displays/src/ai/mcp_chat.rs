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
    let _ = tx.try_send(ChatMessage { id, thread_id: thread_id.to_string(), ts: 0, from, content });
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// ZeroClaw gateway target from env: `ZEROCLAW_GATEWAY_URL` + `ZEROCLAW_GATEWAY_TOKEN`.
pub fn zeroclaw_gateway() -> Option<(String, String)> {
    let url = std::env::var("ZEROCLAW_GATEWAY_URL").ok()?;
    let token = std::env::var("ZEROCLAW_GATEWAY_TOKEN").ok()?;
    let url = url.trim().trim_end_matches('/').to_string();
    let token = token.trim().to_string();
    (!url.is_empty() && !token.is_empty()).then_some((url, token))
}

/// One diagnose turn through the ZeroClaw dispatcher: POST /webhook?agent=<alias>,
/// then emit the agent's final reply. `ZEROCLAW_AGENT` overrides the target alias.
pub async fn zeroclaw_diagnose(prompt: String, thread_id: String, response_tx: Sender<ChatMessage>) {
    let Some((url, token)) = zeroclaw_gateway() else {
        send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Error(
            "ZeroClaw gateway not configured — set ZEROCLAW_GATEWAY_URL and ZEROCLAW_GATEWAY_TOKEN.".into(),
        ));
        send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
        return;
    };
    let agent = std::env::var("ZEROCLAW_AGENT").unwrap_or_else(|_| "diagnostician".into());
    send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Text(format!(
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
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Text(text));
        }
        Ok((status, body)) => {
            let snippet: String = body.chars().take(300).collect();
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Error(format!(
                "ZeroClaw gateway {status}: {snippet}"
            )));
        }
        Err(e) => {
            let msg = if e.is_timeout() {
                "ZeroClaw turn exceeded 900s — check the daemon and gateway request timeout.".to_string()
            } else {
                format!("ZeroClaw gateway unreachable: {e}")
            };
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Error(msg));
        }
    }
    send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
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

fn summarize(args_json: &str) -> String {
    let trimmed = args_json.trim();
    if trimmed.len() <= 80 {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..80])
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
            SentFrom::Gpt => "assistant",
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
            SentFrom::Gpt,
            ChatMessageType::Error(
                "No API key configured. Add one in Account Settings → MCP / AI Endpoint and save.".to_string(),
            ),
        );
        send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
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
                log::error!("stream_chat: tools unavailable ({e}); continuing without tools");
                send(
                    &response_tx,
                    &thread_id,
                    new_id(),
                    SentFrom::Gpt,
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
                send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Error(format!("Request failed: {e}")));
                send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
                break;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Error(format!("HTTP {status}: {detail}")));
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
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
                                send(&response_tx, &thread_id, assistant_id.clone(), SentFrom::Gpt, ChatMessageType::Text(text.to_string()));
                            }
                        }
                    }
                    "response.reasoning.delta"
                    | "response.reasoning_text.delta"
                    | "response.reasoning_summary_text.delta" => {
                        if let Some(text) = delta_text(&json) {
                            if !text.is_empty() {
                                send(&response_tx, &thread_id, think_id.clone(), SentFrom::Gpt, ChatMessageType::Reasoning(text.to_string()));
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
                        send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Error(detail.to_string()));
                    }
                    _ => {}
                }
            }
        }

        if tool_acc.is_empty() {
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
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
                &response_tx,
                &thread_id,
                new_id(),
                SentFrom::Gpt,
                ChatMessageType::Text(format!("{}  {}({})", icons::WRENCH, name, summarize(&args))),
            );
            let result_text = match &mcp_client {
                Some(client) => call_tool(client, &name, &args).await,
                None => "Mastertech tools are not connected.".to_string(),
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

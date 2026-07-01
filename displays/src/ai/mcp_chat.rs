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

/// Streams a chat completion from the OpenAI-compatible endpoint in `mcp_settings`
/// via raw SSE (so the model's `reasoning` tokens are captured — async-openai drops
/// them). Optionally exposes the Mastertech MCP tools and runs the tool-call loop.
/// Emits `Reasoning`, `Text`, and tool-activity messages over `response_tx`.
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
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));

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
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.schema_as_json_value(),
                            }
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

    let mut messages: Vec<serde_json::Value> = Vec::new();
    messages.push(serde_json::json!({ "role": "system", "content": SYSTEM_PROMPT }));
    messages.extend(prior);
    messages.push(serde_json::json!({ "role": "user", "content": input }));

    let http = reqwest::Client::new();

    loop {
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
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
                let Some(delta) = json["choices"].get(0).map(|c| &c["delta"]) else { continue };

                if let Some(content) = delta["content"].as_str() {
                    if !content.is_empty() {
                        send(&response_tx, &thread_id, assistant_id.clone(), SentFrom::Gpt, ChatMessageType::Text(content.to_string()));
                    }
                }
                let reasoning = delta["reasoning"].as_str().or_else(|| delta["reasoning_content"].as_str());
                if let Some(r) = reasoning {
                    if !r.is_empty() {
                        send(&response_tx, &thread_id, think_id.clone(), SentFrom::Gpt, ChatMessageType::Reasoning(r.to_string()));
                    }
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for tc in calls {
                        let idx = tc["index"].as_u64().unwrap_or(0);
                        let entry = tool_acc.entry(idx).or_default();
                        if let Some(id) = tc["id"].as_str() {
                            if !id.is_empty() {
                                entry.0 = id.to_string();
                            }
                        }
                        if let Some(name) = tc["function"]["name"].as_str() {
                            if !name.is_empty() {
                                entry.1 = name.to_string();
                            }
                        }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.2.push_str(args);
                        }
                    }
                }
            }
        }

        if tool_acc.is_empty() {
            send(&response_tx, &thread_id, new_id(), SentFrom::Gpt, ChatMessageType::Done);
            break;
        }

        let mut calls: Vec<(u64, (String, String, String))> = tool_acc.into_iter().collect();
        calls.sort_by_key(|(idx, _)| *idx);

        let tc_json = calls
            .iter()
            .map(|(_, (id, name, args))| {
                serde_json::json!({ "id": id, "type": "function", "function": { "name": name, "arguments": args } })
            })
            .collect::<Vec<_>>();
        messages.push(serde_json::json!({ "role": "assistant", "content": null, "tool_calls": tc_json }));

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
            messages.push(serde_json::json!({ "role": "tool", "tool_call_id": call_id, "content": result_text }));
        }
    }

    drop(mcp_client);
    Ok(())
}

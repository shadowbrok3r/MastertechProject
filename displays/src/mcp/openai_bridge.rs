#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]

use anyhow::{Context, Result};
use futures::{future::pending, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use tokio::net::TcpSocket;

use crate::openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs,
        ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestToolMessageArgs,
        ChatCompletionRequestUserMessageArgs,
        ChatCompletionTool,
        ChatCompletionToolArgs,
        ChatCompletionToolChoiceOption,
        ChatCompletionToolType,
        CreateChatCompletionRequest,
        FunctionObject,
    },
    Client as OpenAIClient,
};

use rmcp::{serve_client, service::{RunningService, Peer, RoleClient}, model::{CallToolRequestParam}};

/// A bridge session that connects OpenAI Chat Completions to an MCP server over TCP.
pub struct OpenAiMcpSession {
    pub oa_client: OpenAIClient<OpenAIConfig>,
    pub model: String,
    mcp_addr: String,
    openai_tools: Vec<ChatCompletionTool>,
    history: VecDeque<ChatCompletionRequestMessage>,
    /// Background task that holds a long‑lived MCP client connection open (keepalive)
    keepalive: Option<tokio::task::JoinHandle<()>>,
    /// Persistent MCP peer for reusing tool calls (avoids opening new TCP sockets per tool)
    mcp_peer: Peer<RoleClient>,
}

impl OpenAiMcpSession {
    /// Connect to the MCP server and create a session. Loads OPENAI_API_KEY via dotenv.
    pub async fn connect(mcp_addr: &str, model: impl Into<String>, system_prompt: Option<String>) -> Result<Self> {
        dotenv::dotenv().ok();
        if std::env::var("OPENAI_API_KEY").is_err() {
            anyhow::bail!("OPENAI_API_KEY environment variable not set.");
        }

        let oa_client = OpenAIClient::new();

        // Establish a single persistent MCP client connection
        let stream = TcpSocket::new_v4()?
            .connect(mcp_addr.parse()?)
            .await
            .with_context(|| format!("Failed to connect to MCP server at {}", mcp_addr))?;
        let running: RunningService<_, ()> = serve_client((), stream).await?;
        let peer = running.peer().clone();
        // Enumerate tools once via persistent peer
        let tools = peer.list_tools(None).await?;
        let openai_tools = Self::convert_mcp_tools(tools)?;

        // Build base history
        let mut history = VecDeque::new();
        if let Some(prompt) = system_prompt {
            let sys = ChatCompletionRequestSystemMessageArgs::default()
                .content(prompt)
                .build()?;
            history.push_back(sys.into());
        }

        // Keep the RunningService alive in background (Single connection model)
        let keepalive = tokio::spawn(async move {
            // Hold the RunningService for the lifetime of this task.
            // When this task is aborted/dropped, the connection is closed.
            let _keep = running;
            // Park forever; no explicit heartbeats needed for local TCP.
            let _ = pending::<()>().await;
        });
        Ok(Self { oa_client, model: model.into(), mcp_addr: mcp_addr.to_string(), openai_tools, history, keepalive: Some(keepalive), mcp_peer: peer })
    }

    // (Removed old list_mcp_tools – now handled inside connect)

    fn convert_mcp_tools(mcp_tools: rmcp::model::ListToolsResult) -> Result<Vec<ChatCompletionTool>> {
        let mut out = Vec::new();
        for t in mcp_tools.tools {
            let parameters_value: Value = serde_json::to_value(&t.input_schema)
                .unwrap_or_else(|_| json!({"type": "object"}));
            let description = t.description.as_ref().map(|c| c.to_string());
            let tool = ChatCompletionToolArgs::default()
                .r#type(ChatCompletionToolType::Function)
                .function(FunctionObject {
                    name: t.name.to_string(),
                    description,
                    parameters: Some(parameters_value),
                    ..Default::default()
                })
                .build()?;
            out.push(tool);
        }
        Ok(out)
    }

    /// Send a user message and drive the model until an assistant response is completed.
    /// If the model emits tool calls, they will be executed against the MCP server and fed back.
    /// If on_delta is provided, it's called with streaming assistant text chunks.
    pub async fn send(&mut self, user_text: &str, mut on_delta: Option<impl FnMut(&str) + Send>) -> Result<String> {
        let user = ChatCompletionRequestUserMessageArgs::default()
            .content(user_text.to_string())
            .build()?;
        self.history.push_back(user.into());
        let base_req = CreateChatCompletionRequest {
            model: self.model.clone(),
            messages: self.history.iter().cloned().collect(),
            tools: if self.openai_tools.is_empty() { None } else { Some(self.openai_tools.clone()) },
            tool_choice: if self.openai_tools.is_empty() { None } else { Some(ChatCompletionToolChoiceOption::Auto) },
            ..Default::default()
        };

        // First pass (non-stream) to detect tool calls
        let resp = self.oa_client.chat().create(base_req.clone()).await?;
        let mut final_answer = String::new();
        if let Some(choice) = resp.choices.into_iter().next() {
            let msg = choice.message;
            if let Some(tool_calls) = msg.tool_calls.clone() {
                // Push assistant tool_calls message using the returned type
                self.history.push_back(ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .tool_calls(tool_calls.clone())
                        .build()?
                        .into(),
                ));

                // Execute tools and push Tool messages
                for call in tool_calls {
                    let args: Value = serde_json::from_str(&call.function.arguments)
                        .unwrap_or_else(|_| json!({}));
                    let tool_output = Self::call_mcp_tool(&self.mcp_addr, &call.function.name, args)
                        .await
                        .with_context(|| format!("Failed calling MCP tool {}", call.function.name))?;
                    self.history.push_back(ChatCompletionRequestMessage::Tool(
                        ChatCompletionRequestToolMessageArgs::default()
                            .content(tool_output)
                            .tool_call_id(call.id)
                            .build()?
                            .into(),
                    ));
                }

                // Second pass to get final assistant answer; stream if requested
                let follow_req = CreateChatCompletionRequest {
                    model: self.model.clone(),
                    messages: self.history.iter().cloned().collect(),
                    ..Default::default()
                };
                if on_delta.is_some() {
                    let mut stream = self.oa_client.chat().create_stream(follow_req).await?;
                    while let Some(ev) = stream.next().await {
                        let resp = ev?;
                        for choice in resp.choices {
                            if let Some(chunk) = choice.delta.content {
                                final_answer.push_str(&chunk);
                                if let Some(cb) = on_delta.as_mut() { cb(&chunk); }
                            }
                        }
                    }
                } else {
                    let follow = self.oa_client.chat().create(follow_req).await?;
                    if let Some(c) = follow.choices.into_iter().next() {
                        if let Some(text) = c.message.content {
                            final_answer = text;
                        }
                    }
                }
            } else if let Some(text) = msg.content {
                // No tool calls: stream if requested for UX
                if on_delta.is_some() {
                    let mut req = base_req.clone();
                    req.stream = Some(true);
                    let mut stream = self.oa_client.chat().create_stream(req).await?;
                    while let Some(ev) = stream.next().await {
                        let resp = ev?;
                        for choice in resp.choices {
                            if let Some(chunk) = choice.delta.content {
                                final_answer.push_str(&chunk);
                                if let Some(cb) = on_delta.as_mut() { cb(&chunk); }
                            }
                        }
                    }
                } else {
                    final_answer = text;
                }
            }
        }

        // Record final assistant message in history
        if !final_answer.is_empty() {
            self.history.push_back(ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessageArgs::default()
                    .content(final_answer.clone())
                    .build()?
                    .into(),
            ));
        }

        Ok(final_answer)
    }

    /// Return the collected chat history (messages sent to OpenAI).
    pub fn history(&self) -> &VecDeque<ChatCompletionRequestMessage> { &self.history }

    async fn call_mcp_tool(addr: &str, name: &str, args: Value) -> Result<String> {
        // Temporary legacy path: open fresh connection (will be phased out)
        let stream = TcpSocket::new_v4()?
            .connect(addr.parse()?)
            .await
            .with_context(|| format!("Failed to connect to MCP server at {}", addr))?;
        let running: RunningService<_, ()> = serve_client((), stream).await?;
        let peer = running.peer().clone();
        use std::borrow::Cow;
        let name_cow: Cow<'static, str> = Cow::Owned(name.to_string());
        let args_obj = match args {
            Value::Object(map) => map,
            other => { let mut map = serde_json::Map::new(); map.insert("input".to_string(), other); map }
        };
        let param = CallToolRequestParam { name: name_cow, arguments: Some(args_obj) };
        let result = peer.call_tool(param).await?;
        Ok(serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string()))
    }
}

impl OpenAiMcpSession {
    /// Request command completions via MCP tool using the persistent peer (no additional sockets)
    pub async fn request_command_completions(&self, partial: &str, shell: &str, context: Option<&str>) -> Result<Vec<crate::mcp::CommandCompletion>> {
        use std::borrow::Cow;
        use rmcp::model::CallToolRequestParam;
        let mut map = serde_json::Map::new();
        map.insert("partial_command".to_string(), Value::String(partial.to_string()));
        map.insert("shell_type".to_string(), Value::String(shell.to_string()));
        if let Some(ctx) = context { map.insert("context".to_string(), Value::String(ctx.to_string())); }
        let param = CallToolRequestParam { name: Cow::Borrowed("complete_command"), arguments: Some(map) };
        let result = self.mcp_peer.call_tool(param).await?;
        let value = serde_json::to_value(&result)?;
        // Recursive search for first 'completions' array
        fn find_completions(v: &Value) -> Option<&Vec<Value>> {
            match v {
                Value::Object(o) => {
                    if let Some(arr) = o.get("completions").and_then(|c| c.as_array()) { return Some(arr); }
                    for (_, val) in o { if let Some(arr) = find_completions(val) { return Some(arr); } }
                    None
                }
                Value::Array(a) => {
                    for val in a { if let Some(arr) = find_completions(val) { return Some(arr); } }
                    None
                }
                _ => None,
            }
        }
        let mut out = Vec::new();
        if let Some(arr) = find_completions(&value) {
            for item in arr.iter() {
                if let Some(comp) = item.get("completion").and_then(|v| v.as_str()) {
                    let description = item.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let confidence = item.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
                    let category = item.get("category").and_then(|v| v.as_str()).map(|s| s.to_string());
                    out.push(crate::mcp::CommandCompletion { completion: comp.to_string(), description, category, confidence });
                }
            }
        }
        Ok(out)
    }
}

impl Drop for OpenAiMcpSession {
    fn drop(&mut self) {
        if let Some(handle) = self.keepalive.take() {
            handle.abort();
        }
    }
}

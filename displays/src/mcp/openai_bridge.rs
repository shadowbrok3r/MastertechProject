#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]

use anyhow::{Context, Result};
use futures::{future::pending, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use tokio::net::TcpSocket;

use crate::{mcp::mcp::ShellType, openai::{
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
}};

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
        log::warn!("send() called with user_text: '{}'", user_text);
        
        let user = ChatCompletionRequestUserMessageArgs::default()
            .content(user_text.to_string())
            .build()?;
        self.history.push_back(user.into());
        
        log::warn!("Building OpenAI request with {} tools available", self.openai_tools.len());
        let base_req = CreateChatCompletionRequest {
            model: self.model.clone(),
            messages: self.history.iter().cloned().collect(),
            tools: if self.openai_tools.is_empty() { None } else { Some(self.openai_tools.clone()) },
            tool_choice: if self.openai_tools.is_empty() { None } else { Some(ChatCompletionToolChoiceOption::Auto) },
            ..Default::default()
        };

        log::warn!("Making first OpenAI API call to detect tool calls...");
        // First pass (non-stream) to detect tool calls
        let resp = self.oa_client.chat().create(base_req.clone()).await?;
        log::warn!("OpenAI API call completed, processing response...");
        
        let mut final_answer = String::new();
        if let Some(choice) = resp.choices.into_iter().next() {
            let msg = choice.message;
            if let Some(tool_calls) = msg.tool_calls.clone() {
                log::info!("AI requested {} tool calls", tool_calls.len());
                
                // Push assistant tool_calls message using the returned type
                self.history.push_back(ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .tool_calls(tool_calls.clone())
                        .build()?
                        .into(),
                ));

                // Execute tools and push Tool messages
                for (i, call) in tool_calls.iter().enumerate() {
                    log::warn!("Executing tool call {}/{}: {} with args: {}", 
                               i + 1, tool_calls.len(), call.function.name, call.function.arguments);
                    
                    let args: Value = serde_json::from_str(&call.function.arguments)
                        .unwrap_or_else(|_| json!({}));
                    
                    // Use persistent peer instead of creating new connections
                    let tool_output = self.call_mcp_tool_persistent(&call.function.name, args)
                        .await
                        .with_context(|| format!("Failed calling MCP tool {}", call.function.name))?;
                    
                    log::warn!("Tool call {} completed with output length: {}", call.function.name, tool_output.len());
                    
                    self.history.push_back(ChatCompletionRequestMessage::Tool(
                        ChatCompletionRequestToolMessageArgs::default()
                            .content(tool_output)
                            .tool_call_id(call.id.clone())
                            .build()?
                            .into(),
                    ));
                }

                log::warn!("All tool calls completed, making second OpenAI API call for final response...");
                // Second pass to get final assistant answer; stream if requested
                let follow_req = CreateChatCompletionRequest {
                    model: self.model.clone(),
                    messages: self.history.iter().cloned().collect(),
                    ..Default::default()
                };
                if on_delta.is_some() {
                    log::warn!("Using streaming response for follow-up");
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
                    log::warn!("Using non-streaming response for follow-up");
                    let follow = self.oa_client.chat().create(follow_req).await?;
                    if let Some(c) = follow.choices.into_iter().next() {
                        if let Some(text) = c.message.content {
                            final_answer = text;
                        }
                    }
                }
                log::warn!("Follow-up response completed with length: {}", final_answer.len());
            } else if let Some(text) = msg.content {
                log::warn!("AI responded directly without tool calls, response length: {}", text.len());
                // No tool calls: stream if requested for UX
                if on_delta.is_some() {
                    log::warn!("Using streaming for direct response");
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
            } else {
                log::warn!("AI response had no content and no tool calls");
            }
        } else {
            log::warn!("No choices returned from OpenAI API");
        }

        // Record final assistant message in history
        if !final_answer.is_empty() {
            log::warn!("Recording final assistant message in history");
            self.history.push_back(ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessageArgs::default()
                    .content(final_answer.clone())
                    .build()?
                    .into(),
            ));
        } else {
            log::warn!("Final answer is empty, not recording in history");
        }

        log::warn!("send() completing with response length: {}", final_answer.len());
        Ok(final_answer)
    }

    /// Return the collected chat history (messages sent to OpenAI).
    pub fn history(&self) -> &VecDeque<ChatCompletionRequestMessage> { &self.history }

    /// Call MCP tool using the persistent peer connection (preferred method)
    async fn call_mcp_tool_persistent(&self, name: &str, args: Value) -> Result<String> {
        log::warn!("call_mcp_tool_persistent: Calling tool '{}' using persistent peer", name);
        log::trace!("call_mcp_tool_persistent: Tool arguments: {}", args);
        
        use std::borrow::Cow;
        let name_cow: Cow<'static, str> = Cow::Owned(name.to_string());
        let args_obj = match args {
            Value::Object(map) => {
                log::warn!("call_mcp_tool_persistent: Using object args directly");
                map
            },
            other => { 
                log::warn!("call_mcp_tool_persistent: Converting non-object args to wrapped object");
                let mut map = serde_json::Map::new(); 
                map.insert("input".to_string(), other); 
                map 
            }
        };
        
        let param = CallToolRequestParam { name: name_cow, arguments: Some(args_obj) };
        log::warn!("call_mcp_tool_persistent: Calling MCP tool with param: {:?}", param);
        
        // Add timeout wrapper around the tool call
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.mcp_peer.call_tool(param)
        ).await
        .map_err(|_| anyhow::anyhow!("MCP tool call timed out after 30 seconds"))?
        .with_context(|| format!("MCP tool call failed for tool: {}", name))?;
        
        log::warn!("call_mcp_tool_persistent: MCP tool call completed successfully");
        log::trace!("call_mcp_tool_persistent: Raw result: {:?}", result);
        
        let result_str = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
        log::warn!("call_mcp_tool_persistent: Serialized result length: {}", result_str.len());
        
        Ok(result_str)
    }

    async fn call_mcp_tool(addr: &str, name: &str, args: Value) -> Result<String> {
        log::warn!("call_mcp_tool: Calling tool '{}' at address '{}'", name, addr);
        log::trace!("call_mcp_tool: Tool arguments: {}", args);
        
        // Temporary legacy path: open fresh connection (will be phased out)
        log::warn!("call_mcp_tool: Opening TCP connection to {}", addr);
        let stream = TcpSocket::new_v4()?
            .connect(addr.parse()?)
            .await
            .with_context(|| format!("Failed to connect to MCP server at {}", addr))?;
        
        log::warn!("call_mcp_tool: TCP connection established, serving client");
        let running: RunningService<_, ()> = serve_client((), stream).await?;
        log::warn!("call_mcp_tool: serve_client completed, getting peer");
        
        let peer = running.peer().clone();
        log::warn!("call_mcp_tool: Peer obtained, preparing call parameters");
        
        use std::borrow::Cow;
        let name_cow: Cow<'static, str> = Cow::Owned(name.to_string());
        let args_obj = match args {
            Value::Object(map) => {
                log::warn!("call_mcp_tool: Using object args directly");
                map
            },
            other => { 
                log::warn!("call_mcp_tool: Converting non-object args to wrapped object");
                let mut map = serde_json::Map::new(); 
                map.insert("input".to_string(), other); 
                map 
            }
        };
        
        let param = CallToolRequestParam { name: name_cow, arguments: Some(args_obj) };
        log::warn!("call_mcp_tool: Calling MCP tool with param: {:?}", param);
        
        // Add timeout wrapper around the tool call
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            peer.call_tool(param)
        ).await
        .map_err(|_| anyhow::anyhow!("MCP tool call timed out after 30 seconds"))?
        .with_context(|| format!("MCP tool call failed for tool: {}", name))?;
        
        log::warn!("call_mcp_tool: MCP tool call completed successfully");
        log::trace!("call_mcp_tool: Raw result: {:?}", result);
        
        let result_str = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
        log::warn!("call_mcp_tool: Serialized result length: {}", result_str.len());
        
        Ok(result_str)
    }
}

impl OpenAiMcpSession {
    /// Get AI-powered command completions via a direct, non-tool-based streaming request.
    pub async fn stream_command_completions(
        &self,
        partial: &str,
        shell: &ShellType,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>
    ) -> Result<Vec<crate::mcp::CommandCompletion>> {
        log::info!("Streaming command completions for partial: '{}', shell: {:?}", partial, shell);

        let prompt = format!(
            "You are a command-line completion assistant. Provide a list of up to 5 command completions for a {shell:?} shell. The user has typed: '{partial}'.
Each completion should be on a new line. Do not add any extra text, explanations, or formatting.
The user wants to append the completion to their existing input, so provide the remaining part of the command.
For example, if the user types 'get' you should return suggestions like: 
Get-CimClass
Get-WmiObject"
        );

        let request = CreateChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("You are a command-line completion assistant who provides only powershell/cmd command suggestions.")
                    .build()?
                    .into(),
                ChatCompletionRequestUserMessageArgs::default()
                    .content(prompt)
                    .build()?
                    .into(),
            ],
            stream: Some(true),
            tools: None, // Explicitly disable tools for this request
            tool_choice: None,
            n: Some(1),
            ..Default::default()
        };

        log::debug!("Sending completion request to OpenAI...");
        let mut stream = self.oa_client.chat().create_stream(request).await?;
        let mut full_response = String::new();

        loop {
            tokio::select! {
                Some(response) = stream.next() => {
                    match response {
                        Ok(chunk) => {
                            if let Some(choice) = chunk.choices.get(0) {
                                if let Some(content) = &choice.delta.content {
                                    full_response.push_str(content);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Error in completion stream: {}", e);
                            break;
                        }
                    }
                }
                _ = &mut cancel_rx => {
                    log::info!("Completion request for '{}' was cancelled.", partial);
                    return Err(anyhow::anyhow!("Request cancelled"));
                }
                else => break,
            }
        }

        log::debug!("Full completion response from AI: \n{}", full_response);

        let completions = full_response
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| crate::mcp::CommandCompletion {
                completion: line.to_string(),
                description: None,
                category: None,
                confidence: 0.9, // Assign a default confidence
            })
            .collect::<Vec<_>>();

        log::info!("Parsed {} completions from stream.", completions.len());
        Ok(completions)
    }

    /// Request command completions via MCP tool using the persistent peer (no additional sockets)
    pub async fn request_command_completions(&self, partial: &str, shell: &ShellType, context: Option<&str>) -> Result<Vec<crate::mcp::CommandCompletion>> {
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

    /// AI-driven command completion using natural conversation. 
    /// The model decides whether to call MCP tools (like complete_command) or respond directly.
    /// This leverages the MCP server properly instead of bypassing it.
    pub async fn ai_command_completions(&mut self, partial: &str, shell: &ShellType) -> Result<Vec<crate::mcp::CommandCompletion>> {
        log::info!("AI command completions requested for partial: '{}', shell: {:?}", partial, shell);
        
        let user_prompt = format!(
            "I need command completions for a {shell:?} shell. The partial command is: '{partial}'\n\nPlease help me complete this command. Use available tools if needed to provide intelligent suggestions."
        );

        log::warn!("Sending prompt to AI: {}", user_prompt);

        // Use the existing send() method which properly handles tool calls
        let response = self.send(&user_prompt, None::<fn(&str)>).await?;
        
        log::warn!("AI response received: {}", response);
        log::warn!("Current history length: {}", self.history.len());
        
        // The MCP complete_command tool should have been called automatically if appropriate,
        // and the results will be in the conversation history. Extract any completion results.
        let completions = self.extract_completions_from_history()?;
        log::info!("Extracted {} completions from history", completions.len());
        
        Ok(completions)
    }

    /// Extract CommandCompletion results from recent MCP tool call responses in history
    fn extract_completions_from_history(&self) -> Result<Vec<crate::mcp::CommandCompletion>> {
        log::warn!("Extracting completions from history, checking {} recent messages", 
                   std::cmp::min(10, self.history.len()));
        
        // Look at recent messages for tool responses containing completion data
        for (i, msg) in self.history.iter().rev().take(10).enumerate() {
            log::trace!("Checking message {}: {:?}", i, msg);
            
            if let ChatCompletionRequestMessage::Tool(tool_msg) = msg {
                log::warn!("Found tool message with ID: {}", tool_msg.tool_call_id);
                
                // Get the content as string
                let content_str = match &tool_msg.content {
                    crate::openai::types::ChatCompletionRequestToolMessageContent::Text(text) => {
                        log::warn!("Tool message content: {}", text);
                        text
                    },
                    other => {
                        log::warn!("Tool message has non-text content: {:?}", other);
                        continue;
                    },
                };
                
                // Try to deserialize tool response directly into our response types
                log::warn!("Attempting to parse as DiagnosticResponse");
                if let Ok(diag_response) = serde_json::from_str::<crate::mcp::DiagnosticResponse>(content_str) {
                    log::warn!("Successfully parsed as DiagnosticResponse: {:?}", diag_response);
                    if let crate::mcp::DiagnosticResponse::CommandCompletions { completions, .. } = diag_response {
                        log::info!("Found {} command completions in DiagnosticResponse", completions.len());
                        return Ok(completions);
                    }
                } else {
                    log::warn!("Failed to parse as DiagnosticResponse");
                }
                
                // Fallback: try to parse as raw MCP CallToolResult containing completions
                log::warn!("Attempting to parse as CallToolResult");
                if let Ok(call_result) = serde_json::from_str::<rmcp::model::CallToolResult>(content_str) {
                    log::warn!("Successfully parsed as CallToolResult");
                    // The MCP result content should contain our completion data
                    if let Some(content_vec) = call_result.content {
                        log::warn!("CallToolResult has {} content items", content_vec.len());
                        for (j, content) in content_vec.iter().enumerate() {
                            log::trace!("Checking content item {}: {:?}", j, content);
                            // Try to extract from each content item
                            if let Ok(json_value) = serde_json::to_value(&content) {
                                if let Ok(response) = serde_json::from_value::<crate::mcp::DiagnosticResponse>(json_value) {
                                    if let crate::mcp::DiagnosticResponse::CommandCompletions { completions, .. } = response {
                                        log::info!("Found {} command completions in CallToolResult content", completions.len());
                                        return Ok(completions);
                                    }
                                }
                            }
                        }
                    } else {
                        log::warn!("CallToolResult has no content");
                    }
                } else {
                    log::warn!("Failed to parse as CallToolResult");
                }
            }
        }
        
        log::warn!("No command completions found in conversation history");
        Ok(Vec::new())
    }
}

impl Drop for OpenAiMcpSession {
    fn drop(&mut self) {
        if let Some(handle) = self.keepalive.take() {
            handle.abort();
        }
    }
}

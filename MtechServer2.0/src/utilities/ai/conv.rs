use super::oa_client::OaClient;
use super::tools::AiTools;
use crate::utilities::ai::{chat, gpts};
use anyhow::{Error, Result};
use async_openai_wasm::types::{
    ChatChoice, ChatCompletionRequestMessage, ChatCompletionToolChoiceOption,
    CreateChatCompletionRequest,
};
use serde_json::Value;

pub async fn send_user_msg(
    oa_client: OaClient,
    ai_tools: Option<AiTools>, // Make ai_tools optional for flexibility
    messages: Vec<ChatCompletionRequestMessage>,
    existing_thread_id: Option<String>, // Optional thread ID for ongoing conversations
) -> Result<Vec<ChatChoice>, Error> {
    let chat_client = oa_client.chat();
    let model = gpts::MODEL;

    // Extract tools and rpc_router if provided
    let (rpc_router, tools) = if let Some(ai_tools) = ai_tools {
        (Some(ai_tools.router()), Some(ai_tools.chat_tools_clone()))
    } else {
        (None, None)
    };

    // -- Exec Chat Request
    let msg_req = CreateChatCompletionRequest {
        model: model.to_string(),
        messages: messages.clone(),
        tools: tools.clone(),
        tool_choice: Some(ChatCompletionToolChoiceOption::Auto),
        ..Default::default()
    };

    // If there's an existing thread ID, add to that thread; otherwise, create a new request
    let chat_response = if let Some(thread_id) = existing_thread_id {
        chat_client
            .threads()
            .messages(&thread_id)
            .create(msg_req)
            .await?
    } else {
        chat_client.create(msg_req).await?
    };

    let first_choice = chat::first_choice(chat_response)?;

    // -- If message.content, end early
    if first_choice.message.content.is_some() {
        return Ok(vec![first_choice.clone()]);
    }

    // -- Otherwise, get/call tools/rpc calls and capture the Tool Responses
    struct ToolResponse {
        tool_call_id: String,
        response: Value,
    }
    let mut tool_responses: Vec<ToolResponse> = Vec::new();

    if let Some(rpc_router) = rpc_router {
        if let Some(tool_calls) = first_choice.message.tool_calls {
            for tool_call in tool_calls.iter().flatten() {
                let tool_call_id = tool_call.id.clone();
                let fn_name = tool_call.function.name.clone();
                let params: Value = serde_json::from_str(&tool_call.function.arguments)?;

                // Execute with rpc_router
                let call_result = rpc_router
                    .call_route(None, fn_name, Some(params))
                    .await
                    .unwrap();
                let response = call_result.value;

                // Add it to the tool_responses
                tool_responses.push(ToolResponse {
                    tool_call_id,
                    response,
                });
            }
        }
    }

    // -- Make messages mutable for follow-up
    let mut messages = messages;

    // -- Append the tool calls (send from AI Model)
    if let Some(tool_calls) = first_choice.message.tool_calls {
        messages.push(chat::tool_calls_msg(tool_calls)?);
    }

    // -- Append the Tool Responses (computed by this code)
    for ToolResponse {
        tool_call_id,
        response,
    } in tool_responses
    {
        messages.push(chat::tool_response_msg(tool_call_id, response)?);
    }

    // -- Exec second request with tool responses
    let msg_req = CreateChatCompletionRequest {
        model: model.to_string(),
        messages,
        tools,
        tool_choice: Some(ChatCompletionToolChoiceOption::Auto),
        ..Default::default()
    };

    let chat_response = if let Some(thread_id) = existing_thread_id {
        chat_client
            .threads()
            .messages(&thread_id)
            .create(msg_req)
            .await?
    } else {
        chat_client.create(msg_req).await?
    };

    let all_choices = chat::all_choices(chat_response)?;

    Ok(all_choices)
}

/*
pub async fn send_user_msg(
    oa_client: OaClient,
    ai_tools: AiTools,
    messages: Vec<ChatCompletionRequestMessage>,
) -> Result<Vec<ChatChoice>, Error> {
    let chat_client = oa_client.chat();
    let model = gpts::MODEL;

    // -- Extract tools and rpc_router
    let rpc_router = ai_tools.router();
    let tools = Some(ai_tools.chat_tools_clone());

    // -- Exec Chat Request
    let msg_req = CreateChatCompletionRequest {
        model: model.to_string(),
        messages: messages.clone(),
        tools: tools.clone(),
        tool_choice: Some(ChatCompletionToolChoiceOption::Auto),
        ..Default::default()
    };
    let chat_response = chat_client.create(msg_req).await?;
    let first_choice = chat::first_choice(chat_response)?;

    // -- If message.content, end early
    if first_choice.message.content.is_some() {
        return Ok(vec![first_choice.clone()]);
    }

    // -- Otherwise, get/call tools/rpc calls and capture the Tool Responses
    struct ToolResponse {
        tool_call_id: String,
        /// Response value of the rpc_router call
        response: Value,
    }
    let mut tool_responses: Vec<ToolResponse> = Vec::new();

    // For each tool_call, rpc_router call
    let tool_calls = first_choice.message.tool_calls;
    for tool_call in tool_calls.iter().flatten() {
        let tool_call_id = tool_call.id.clone();
        let fn_name = tool_call.function.name.clone();
        let params: Value = serde_json::from_str(&tool_call.function.arguments)?;

        // Execute with rpc_router
        let call_result = rpc_router
            .call_route(None, fn_name, Some(params))
            .await
            .unwrap();
        let response = call_result.value;

        // Add it to the tool_responses
        tool_responses.push(ToolResponse {
            tool_call_id,
            response,
        });
    }

    // -- Make messages mutable for follow-up
    let mut messages = messages;

    // -- Append the tool calls (send from AI Model)
    if let Some(tool_calls) = tool_calls {
        messages.push(chat::tool_calls_msg(tool_calls)?);
    }

    // -- Append the Tool Responses (computed by this code)
    for ToolResponse {
        tool_call_id,
        response,
    } in tool_responses
    {
        messages.push(chat::tool_response_msg(tool_call_id, response)?);
    }

    // -- Exec second request with tool responses
    let msg_req = CreateChatCompletionRequest {
        model: model.to_string(),
        messages,
        tools,
        tool_choice: Some(ChatCompletionToolChoiceOption::Auto),
        ..Default::default()
    };
    let chat_response = chat_client.create(msg_req).await?;
    // let first_choice = chat::first_choice(chat_response)?;
    let all_choices = chat::all_choices(chat_response)?;

    // -- Get the final response
    // let content = first_choice.message.content.unwrap();

    Ok(all_choices)
}
*/

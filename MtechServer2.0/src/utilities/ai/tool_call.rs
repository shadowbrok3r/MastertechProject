use crate::utilities::ai::{conv, tools::AiTools};
use anyhow::{Error, Result};
use async_openai_wasm::types::{
    ChatChoice, ChatCompletionToolChoiceOption, CreateChatCompletionRequest,
};
use database::DATABASE;
use log::info;
use rpc_router::{router_builder, RpcParams};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use surrealdb::RecordId;

use super::{chat, gpts, oa_client::new_oa_client};

pub async fn call() -> Result<(), Error> {
    // -- Init AI Client
    let oa_client = new_oa_client()?;

    let chat_client = oa_client.chat();
    let model = gpts::MODEL.to_string();

    // -- User input
    let input = "What is the weather in the California's best city and Paris?";

    // -- Build messages
    let messages = vec![chat::user_msg(input)?];

    // -- Build tools
    let tool_weather = chat::tool_fn(
        "get_weather",
        "get the weather for a city",
        json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city and state, e.g. San Francisco, CA"
                },
                "country": {
                    "type": "string",
                    "description": "The full country name of the city"
                },
                "unit": {
                    "type": "string", "enum": ["celsius", "fahrenheit"],
                    "description": "Unit respecting the country of the city"
                },
            },
            "required": ["location", "country", "unit"],
        }),
    )?;
    let tools = Some(vec![tool_weather]);

    // -- Exec Chat Request
    let msg_req = CreateChatCompletionRequest {
        model,
        messages,
        tools,
        tool_choice: Some(ChatCompletionToolChoiceOption::Auto),
        ..Default::default()
    };
    let chat_response = chat_client.create(msg_req).await?;
    let first_choice = chat::first_choice(chat_response)?;

    // -- Extract and print the tool calls
    if let Some(tool_calls) = first_choice.message.tool_calls {
        for tool in tool_calls {
            info!(
                r#"
    ===   function: '{}'
        arguments: {}"#,
                tool.function.name, tool.function.arguments
            );
        }
    }
    Ok(())
}

#[allow(unused)] // Will be passthrough API
#[derive(Debug, Deserialize, RpcParams)]
struct GetWeatherParams {
    location: String,
    country: String,
    unit: String,
}

#[derive(Serialize)]
struct Weather {
    temperature: f64,
    unit: String,
    humidity_rh: f32,
}

async fn get_weather(params: GetWeatherParams) -> Result<Weather, String> {
    Ok(Weather {
        temperature: 30.,
        unit: params.unit,
        humidity_rh: 0.3,
    })
}
#[derive(Debug, Deserialize, RpcParams)]
pub struct GetTaskSummaryParams {
    /// The ID of the task you want to fetch
    pub task_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: RecordId,
    pub task_name: String,
    pub task_description: String,
}

pub async fn get_task_summary(params: GetTaskSummaryParams) -> Result<TaskSummary, String> {
    gloo_console::info!(format!("Calling task"));
    let task: Option<TaskSummary> = DATABASE
        .query("SELECT id, task_name, task_description FROM task WHERE service_number == $task_id")
        .bind(("task_id", params.task_id))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;
    gloo_console::info!(format!("{task:?}"));
    Ok(task.unwrap())
}

pub async fn call_with_response(input: &str) -> Result<Vec<ChatChoice>, Box<Error>> {
    // -- Init AI Client
    let oa_client = new_oa_client()?;
    let chat_client = oa_client.chat();
    let model = gpts::MODEL;
    // Add a system message to instruct the model to use Markdown formatting
    let system_message = chat::user_msg(
        r#"
				You are an AI assistant. 
				You should use Markdown language to format your responses whenever applicable. 
				You can use headers (With one # followed by a space), lists, code blocks, bold, italics, and other Markdown 
				features to make the response more readable.
		"#,
    )?;

    let user_message = chat::user_msg(input)?;

    let messages = vec![system_message, user_message];

    let tool_task_summary = chat::tool_fn(
        "get_task_summary",
        "get the summary of a task given its ID",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The ID of the task you want to fetch"
                }
            },
            "required": ["task_id"],
        }),
    )?;

    // Add both tools to the list
    let tools = Some(vec![tool_task_summary]);

    // -- Init rpc_router
    let rpc_router = router_builder![get_task_summary].build();

    // -- Exec Chat Request
    let msg_req = CreateChatCompletionRequest {
        model: model.to_string(),
        messages: messages.clone(),
        tools: tools.clone(),
        tool_choice: Some(ChatCompletionToolChoiceOption::Auto),
        ..Default::default()
    };
    let chat_response = chat_client.create(msg_req).await.unwrap();
    let first_choice = chat::first_choice(chat_response)?;

    // -- If message.content, end early
    if let Some(response_content) = first_choice.message.content {
        gloo_console::info!(format!(
            "\nResponse early (no tools):\n\n{response_content}"
        ));
        // return Ok(res);
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
        let params: Value = serde_json::from_str(&tool_call.function.arguments).unwrap();

        gloo_console::info!(format!(
            "Params: {params:?}\ntool_call_id: {tool_call_id:?}\nfn_name: {fn_name:?}"
        ));
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
    let chat_response = chat_client.create(msg_req).await.unwrap();
    let choices: Vec<ChatChoice> = chat::all_choices(chat_response)?;

    Ok(choices)
}

pub async fn call_with_response_ai_tools(input: &str) -> Result<Vec<ChatChoice>, Box<Error>> {
    // -- Init AI Client
    let oa_client = new_oa_client()?;

    // Add a system message to instruct the model to use Markdown formatting
    let system_message = chat::user_msg(
        r#"
				You are an AI assistant. 
				You should use Markdown language to format your responses whenever applicable. 
				You can use headers (With one # followed by a space), lists, code blocks, bold, italics, and other Markdown 
				features to make the response more readable.
		"#,
    )?;

    let user_message = chat::user_msg(input)?;

    let messages = vec![system_message, user_message];

    let tool_task_summary = chat::tool_fn(
        "get_task_summary",
        "get the summary of a task given its ID",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The ID of the task you want to fetch"
                }
            },
            "required": ["task_id"],
        }),
    )?;

    // Add both tools to the list
    let tools = vec![tool_task_summary];

    // -- Init rpc_router
    let rpc_router = router_builder![get_task_summary].build();

    let ai_tools = AiTools::new(rpc_router, tools);
    // -- Execute question with conv
    let response: Vec<ChatChoice> = conv::send_user_msg(oa_client, ai_tools, messages).await?;

    println!("\nFinal answer:\n\n{response:?}");

    Ok(response)
}

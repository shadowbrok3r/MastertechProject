use crate::utilities::ai::{conv, tools::AiTools};
use anyhow::{Error, Result};
use async_openai_wasm::{
    config::OpenAIConfig,
    types::{
        AssistantStreamEvent, ChatChoice, ChatCompletionToolChoiceOption,
        CreateChatCompletionRequest, CreateMessageRequestArgs, CreateRunRequestArgs,
        CreateThreadRequestArgs, MessageContent, MessageDeltaContent, MessageRole, RunObject,
        RunStatus, SubmitToolOutputsRunRequest, ToolsOutputs,
    },
    Client,
};
use database::DATABASE;
use futures::StreamExt;
use gloo_timers::future::sleep;
use log::info;
use rpc_router::{router_builder, RpcParams};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use surrealdb::RecordId;
use wasm_bindgen_futures::spawn_local;

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
        gloo_console::info!(format!("Response early (no tools):\n\n{response_content}"));
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

    let user_message = chat::user_msg(input)?; // ChatCompletionRequestMessage

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
    )?; // ChatCompletionTool

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
/*
pub async fn assistant_call_with_response_ai_tools(
    input: &str,
    existing_thread_id: Option<String>,
) -> Result<Vec<ChatChoice>, Box<Error>> {
    // -- Initialize AI Client
    let oa_client = new_oa_client()?;
    let assistant_id = "asst_3wOgem2DpYiXkk7x34hVb9My"; // Your existing assistant ID

    // -- Check if there is an existing thread to use, or create a new one
    let thread_id = if let Some(thread_id) = existing_thread_id {
        thread_id
    } else {
        // Create a new thread if none exists
        let thread_request = CreateThreadRequestArgs::default().build()?;
        let thread = oa_client.threads().create(thread_request.clone()).await?;
        thread.id
    };

    // -- Add User Message to the Thread
    let user_message = CreateMessageRequestArgs::default()
        .role(MessageRole::User)
        .content(input.to_string())
        .build()?;

    oa_client
        .threads()
        .messages(&thread_id)
        .create(user_message)
        .await?;

    // -- Create a Run for the Thread
    let run_request = CreateRunRequestArgs::default()
        .assistant_id(assistant_id)
        .stream(false) // Set stream to false for a more synchronous response
        .build()?;

    let run = oa_client
        .threads()
        .runs(&thread_id)
        .create(run_request)
        .await?;

    // -- Wait for the Run to Complete
    let mut awaiting_response = true;
    while awaiting_response {
        // Retrieve the Run
        let run_status = oa_client
            .threads()
            .runs(&thread_id)
            .retrieve(&run.id)
            .await?;

        // Check the Status of the Run
        match run_status.status {
            RunStatus::Completed => {
                awaiting_response = false;
            }
            RunStatus::Failed => {
                return Err(format!("Run Failed: {:#?}", run_status).into());
            }
            RunStatus::Queued
            | RunStatus::InProgress
            | RunStatus::Cancelling
            | RunStatus::Incomplete => {
                println!("--- Run In Progress ...");
                // Wait for 1 second before checking the status again
                sleep(web_time::Duration::from_secs(1)).await;
            }
            RunStatus::Cancelled | RunStatus::Expired | RunStatus::RequiresAction => {
                return Err(format!("Run Error: Status - {:?}", run_status.status).into());
            }
        }
    }

    // -- Retrieve the Response from the Run
    let query = [("limit", "1")]; // Limit the list responses to 1 message
    let response = oa_client
        .threads()
        .messages(&thread_id)
        .list(&query)
        .await?;

    // -- Map the Response into Vec<ChatChoice>
    let chat_choices: Vec<ChatChoice> = response
        .data
        .iter()
        .filter_map(|message| {
            if let Some(MessageContent::Text(text)) = message.content.first() {
                Some(ChatChoice {
                    message: text.text.value.clone(),
                })
            } else {
                None
            }
        })
        .collect();

    // -- Return the Assistant's Response and the Thread ID for Subsequent Messages
    Ok(chat_choices)
}
*/
const SYSTEM_INSTRUCTIONS: &str = r#"
Analyze diagnostic data from repairs conducted by the company to identify trends and correlations, and assist in understanding which products, models, or hardware configurations are associated with the most issues.

In this task, you will provide a statistical analysis of the repair data, generate visualizations to illustrate trends, and run statistical formulas to derive meaningful insights that help to identify recurring problems. 
Your aim is to help the user understand which products or configurations are the most problematic. The analysis should include relevant statistical metrics, visual trends, and clear interpretations of what the data reveals.

# Steps

1. **Data Overview**:
   - Review the dataset provided, inspect its structure, and identify important fields (e.g., product type, model, configuration, repair frequency).
   - Summarize key statistics: counts, percentages, averages, etc.

2. **Identify Key Variables**:
   - List important categories that should be analyzed, such as product type, model number, hardware configuration, repair type, etc.
  
3. **Statistical Analysis and Correlation**:
   - Use statistical methods to identify factors most correlated with repair frequency, such as:
     - Frequency counts of repairs per product type.
     - Calculating failure rates for different models (failures/total units serviced).
     - Performing correlation analysis between configurations (e.g., RAM, GPU types) and uptick in failures.
  
4. **Graph Generation**:
   - Generate descriptive graphs such as:
     - **Bar Graphs/Histograms**: Frequency of issues per product or configuration type.
     - **Scatter Plots**: Showing correlations between hardware configurations and repair frequency.
     - **Pie Charts**: Percentage of total failures categorized by model, product type, etc.
     - Ensure that the graphs are easy to understand and provide insightful trends.

5. **Provide Insights**:
   - Interpret the analysis and explain what the trends and correlations suggest.
   - Help answer questions like:
     - Which product models tend to have higher repair rates?
     - Which type of configuration seems the most prone to failure?
     - Are there any specific time patterns in the data (e.g., seasonal failure rates)?

6. **Recommendations**:
   - When possible, offer actionable recommendations based on your findings, such as which configurations to avoid or need improvement.

# Output Format

Provide the output as a comprehensive report consisting of:
- Key insights summarized in bullet points or short paragraphs.
- Graphs (where applicable, provide a link to the graph or a description of the trends revealed).
- Identified correlations and their likely interpretations.
- Suggested actionable recommendations.

The output report should be structured as follows:
1. **Overview**: A summary of the key findings.
2. **Graphs & Visualizations**: Include graphs with brief explanations.
3. **Statistical Analysis**: Present the calculated correlations, trends, insights, and notable patterns.
4. **Recommendations**: A list of actionable recommendations based on the analysis.

# Example

**Input Data**:
Repair records for multiple products with attributes including:
- Product Type: Laptop, Desktop
- Model: Specific model identifier
- Configuration: RAM Size, Disk Type, Processor, etc.
- Repair Record: Description & date of repair

**Output** (Summarized):
1. **Overview**:
   - Laptop Model X has a notably higher frequency of repairs.
   - Systems with Configuration Y experienced 30% higher failures compared to Configuration Z.

2. **Graphs & Visualizations**:
   - [Graph 1: Bar Graph showing repair frequency per product model]
   - [Graph 2: Pie Chart showing repair percentage by configuration types]

3. **Statistical Analysis**:
   - There is a significant positive correlation (correlation coefficient = 0.78) between 8GB RAM configuration and failure rate.
   - Laptops tend to have an 18% higher failure rate compared to desktops.

4. **Recommendations**:
   - Consider shifting to 16GB RAM on Laptop Model X due to reduced failure rates observed.
   - Further analyze storage type's effects on repair rates.

# Notes

- Avoid conclusions without statistical backing.
- Be cautious about overinterpreting correlations—correlation does not imply causation.
- If identifying outliers, visually differentiate them in graphs for clarity."#;

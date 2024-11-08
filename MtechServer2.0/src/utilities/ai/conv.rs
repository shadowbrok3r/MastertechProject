use super::oa_client::OaClient;
use super::tools::AiTools;
use crate::utilities::ai::{chat, gpts};
use anyhow::{Error, Result};
use async_openai_wasm::types::{
    ChatChoice, ChatCompletionRequestMessage, ChatCompletionToolChoiceOption,
    CreateChatCompletionRequest, CreateMessageRequestArgs, CreateRunRequestArgs,
    CreateThreadRequestArgs, MessageContent, RunStatus,
};
use serde_json::Value;

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

pub async fn send_assistant_msg(
    oa_client: OaClient,
    ai_tools: AiTools,                           // Ability to use tools retained
    messages: Vec<ChatCompletionRequestMessage>, // Consistent with send_user_msg
    existing_thread_id: Option<String>,          // Optional thread ID for existing conversations
) -> Result<(Vec<ChatChoice>, String), Error> {
    let assistant_id = "asst_3wOgem2DpYiXkk7x34hVb9My"; // Your existing assistant ID

    // Step 1: Determine if we are using an existing thread or creating a new one
    let thread_id = if let Some(thread_id) = existing_thread_id {
        thread_id
    } else {
        // Create a new thread if none exists
        let thread_request = CreateThreadRequestArgs::default().build()?;
        let thread = oa_client.threads().create(thread_request.clone()).await?;
        thread.id
    };

    // Step 2: Add messages to the thread (each message in `messages` vector)
    for message in messages {
        let create_message_request = CreateMessageRequestArgs::default()
            .role(message.role)
            .content(message.content.clone())
            .build()?;

        oa_client
            .threads()
            .messages(&thread_id)
            .create(create_message_request)
            .await?;
    }

    // Step 3: Create a Run for the Thread
    let run_request = CreateRunRequestArgs::default()
        .assistant_id(assistant_id)
        .stream(false) // Set stream to false for a more synchronous response
        .build()?;

    let run = oa_client
        .threads()
        .runs(&thread_id)
        .create(run_request)
        .await?;

    // Step 4: Wait for the Run to Complete
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
                sleep(web_time::Duration::from_secs(1)).await;
            }
            RunStatus::Cancelled | RunStatus::Expired | RunStatus::RequiresAction => {
                return Err(format!("Run Error: Status - {:?}", run_status.status).into());
            }
        }
    }

    // Step 5: Retrieve the Response from the Run
    let query = [("limit", "1")]; // Limit the list responses to 1 message
    let response = oa_client
        .threads()
        .messages(&thread_id)
        .list(&query)
        .await?;

    // Step 6: Map the Response into Vec<ChatChoice>
    let chat_choices: Vec<ChatChoice> = response
        .data
        .iter()
        .filter_map(|message| {
            if let Some(MessageContent::Text(text)) = message.content.first() {
                Some(ChatChoice {
                    message: text.text.value.clone(),
                    ..Default::default()
                })
            } else {
                None
            }
        })
        .collect();

    // Step 7: Return the Assistant's Response and the Thread ID for Subsequent Messages
    Ok((chat_choices, thread_id))
}

pub const SYSTEM_INSTRUCTIONS: &str = r#"
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

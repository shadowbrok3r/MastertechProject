
use rpc_router::{router_builder, RouterBuilder, RpcParams};
use crate::{ai::{chat, tools::tasks::get_task_summary}, openai::types::chat::ChatCompletionTool};
use serde::{Deserialize, Serialize};
use anyhow::{Error, Result};

use super::tasks::GetTaskSummaryParams;

pub fn router_builder() -> RouterBuilder {
    router_builder![get_task_summary].into()
}

pub fn chat_tools() -> Result<Vec<ChatCompletionTool>, Error> {
    // let tool_weather = chat::tool_fn_from_type::<GetWeatherParams>()?;
    let tool_task_summary = chat::tool_fn_from_type::<GetTaskSummaryParams>()?;
    Ok(vec![tool_task_summary])
}

/// # get_weather
/// get the weather for a city
#[allow(unused)] // Will be passthrough API
#[derive(Debug, Deserialize, RpcParams, schemars::JsonSchema)]
struct GetWeatherParams {
    /// The city and state, e.g. San Francisco, CA
    location: String,
    /// The full country name of the city
    country: String,
    /// Unit respecting the country of the city
    unit: TempUnit,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, RpcParams)]
enum TempUnit {
    Celcius,
    Fahrenheit,
}

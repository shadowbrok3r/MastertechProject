use crate::utilities::ai::{chat::tool_fn, model::ModelManager};
use anyhow::{Error, Result};
use async_openai_wasm::types::ChatCompletionTool;
use database::DATABASE;
use rpc_router::RpcParams;
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;

use super::{tool_spec, ToolSpec};

pub fn tool_spec_task_summary() -> Result<ToolSpec, Error> {
    tool_spec::<GetTaskSummaryParams>()
}

pub fn tool_fn_get_task_summary() -> Result<ChatCompletionTool> {
    let spec = tool_spec_task_summary()?;
    tool_fn(spec.fn_name, spec.fn_description, spec.params)
}

#[derive(Debug, Deserialize, RpcParams, schemars::JsonSchema)]
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

pub async fn get_task_summary(
    _mm: ModelManager,
    params: GetTaskSummaryParams,
) -> Result<TaskSummary, String> {
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

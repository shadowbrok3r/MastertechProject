use rmcp::{
    handler::server::{tool::{Parameters, ToolRouter}, ServerHandler},
    model::{CallToolResult, Content, ErrorCode, ErrorData, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
// no direct serde_json imports needed; payloads are constructed in tools module
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
use tokio::time::{sleep, Duration};
// uuid is used in tools helpers, not directly here
use super::tools::{
    mcp_analyze_bsod,
    mcp_analyze_event_logs,
    mcp_generate_performance_report,
    mcp_get_system_summary,
    mcp_complete_command,
    mcp_execute_script,
    mcp_wait as mcp_wait_payload,
};

// --- Shared domain types (pared down to what tools need) ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventLogTimeRange {
    pub hours_back: u32,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")] 
pub enum EventLogSeverity { Critical, Error, Warning, Information, Verbose }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")] 
pub enum ScriptType { PowerShell, Batch, Python, Bash }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")] 
pub enum ShellType { Cmd, PowerShell, Bash, Zsh, Fish }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")] 
pub enum RiskLevel { Low, Medium, High, Critical }

// --- Tool Parameter Structs ---

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeBsodParams {
    #[schemars(description = "Specific dump file to analyze (optional)")]
    pub dump_path: Option<String>,
    #[schemars(description = "Also scan common locations for recent dumps", default = "default_true")]
    pub include_recent: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeEventLogsParams {
    #[schemars(description = "Event log name")]
    pub log_name: Option<String>,
    pub time_range: Option<EventLogTimeRange>,
    pub severity: Option<EventLogSeverity>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePerformanceReportParams {
    #[schemars(description = "Hours of history to analyze", default = "default_24")]
    pub duration_hours: Option<u32>,
    #[schemars(description = "Include per‑process analysis", default = "default_true")]
    pub include_processes: Option<bool>,
    #[schemars(description = "Include hardware telemetry", default = "default_true")]
    pub include_hardware: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetSystemSummaryParams {
    #[schemars(default = "default_true")]
    pub include_hardware: Option<bool>,
    #[schemars(default = "default_true")]
    pub include_software: Option<bool>,
    #[schemars(default = "default_true")]
    pub include_network: Option<bool>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompleteCommandParams {
    #[schemars(description = "Partial command to complete")]
    pub partial_command: String,
    #[schemars(description = "Shell dialect", default = "default_shell")]
    pub shell_type: Option<ShellType>,
    #[schemars(description = "Free‑form context for better completions")]
    pub context: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteScriptParams {
    pub script: String,
    pub script_type: ScriptType,
    #[schemars(description = "Human‑readable purpose of the script")]
    pub description: String,
    #[schemars(description = "Require approval before execution", default = "default_true")]
    pub require_approval: Option<bool>,
}

#[inline]
fn default_true() -> Option<bool> { Some(true) }
#[inline]
fn default_24() -> Option<u32> { Some(24) }
#[inline]
fn default_shell() -> Option<ShellType> { Some(ShellType::Cmd) }

// --- Provider ---

#[derive(Clone)]
pub struct DiagnosticToolProvider {
    tool_router: ToolRouter<Self>,
}

impl DiagnosticToolProvider {
    pub fn new() -> Self { Self { tool_router: Self::tool_router() } }
}

// --- Tool implementations ---

#[tool_router]
impl DiagnosticToolProvider {
    /// Analyze Windows Blue Screen dump(s).
    #[tool(name = "analyze_bsod", description = "Analyze Windows BSOD dump files for causes and recommendations.")]
    async fn analyze_bsod(
        &self,
        Parameters(AnalyzeBsodParams { dump_path, include_recent }): Parameters<AnalyzeBsodParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = mcp_analyze_bsod(dump_path, include_recent.unwrap_or(true))
            .await
            .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }

    /// Parse Windows Event Logs and surface patterns.
    #[tool(name = "analyze_event_logs", description = "Parse and analyze Windows Event Viewer logs for patterns and issues.")]
    async fn analyze_event_logs(
        &self,
        Parameters(params): Parameters<AnalyzeEventLogsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = mcp_analyze_event_logs(
            params.log_name,
            params.time_range.as_ref().map(|t| t.hours_back),
            params.severity.as_ref().map(|s| format!("{:?}", s)),
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }

    /// Build a performance report over a time window.
    #[tool(name = "generate_performance_report", description = "Generate comprehensive system performance analysis reports.")]
    async fn generate_performance_report(
        &self,
        Parameters(p): Parameters<GeneratePerformanceReportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = mcp_generate_performance_report(
            p.duration_hours,
            p.include_processes,
            p.include_hardware,
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }

    /// Summarize system state and health.
    #[tool(name = "get_system_summary", description = "Generate overall system health and configuration summary.")]
    async fn get_system_summary(
        &self,
        Parameters(p): Parameters<GetSystemSummaryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = mcp_get_system_summary(
            p.include_hardware,
            p.include_software,
            p.include_network,
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }

    /// Provide intelligent shell completions.
    #[tool(name = "complete_command", description = "Provide intelligent command completions for various shells.")]
    async fn complete_command(
        &self,
        Parameters(p): Parameters<CompleteCommandParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = mcp_complete_command(
            p.partial_command,
            format!("{:?}", p.shell_type.unwrap_or(ShellType::Cmd)).to_lowercase(),
            p.context,
        )
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }

    /// Execute a script with an approval workflow.
    #[tool(name = "execute_script", description = "Execute diagnostic scripts with approval workflow.")]
    async fn execute_script(
        &self,
        Parameters(p): Parameters<ExecuteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = mcp_execute_script(
            p.script,
            format!("{:?}", p.script_type),
            p.description,
            p.require_approval,
        )
        .await
        .map_err(to_internal)?;
        Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }

    /// Simple wait tool useful for orchestration.
    #[tool(name = "wait", description = "Wait/sleep for the specified milliseconds (default 2000ms).")]
    async fn wait_tool(
        &self,
        Parameters(DurationParams { duration_ms }): Parameters<DurationParams>,
    ) -> Result<CallToolResult, ErrorData> {
    let ms = duration_ms.unwrap_or(2000);
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    sleep(Duration::from_millis(ms)).await;
    let payload = mcp_wait_payload(Some(ms)).await.map_err(to_internal)?;
    Ok(CallToolResult::success(vec![Content::json(payload).map_err(to_internal)?]))
    }
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DurationParams { #[schemars(default)] duration_ms: Option<u64> }

#[allow(dead_code)]
fn assess_script_risk(script: &str) -> RiskLevel {
    let s = script.to_lowercase();
    if s.contains("format") || s.contains(" del ") || s.contains("rm -rf") || s.contains("regedit") { return RiskLevel::Critical; }
    if s.contains("install") || s.contains("service") || s.contains("config") { return RiskLevel::Medium; }
    RiskLevel::Low
}

fn to_internal<E: std::fmt::Display>(e: E) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
}

// --- Server metadata ---
const INSTRUCTIONS: &str = "Mastertech Diagnostics – rmcp tools bundle. Use provided tools to analyze Windows systems. Avoid destructive actions without explicit approval.";

#[tool_handler]
impl ServerHandler for DiagnosticToolProvider {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder().enable_tools().enable_experimental().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(INSTRUCTIONS.to_string()),
        }
    }
}

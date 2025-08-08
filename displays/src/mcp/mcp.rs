use rmcp::{
    handler::server::{tool::{Parameters, ToolRouter}, ServerHandler},
    model::{CallToolResult, Content, ErrorCode, ErrorData, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

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
        let include_recent = include_recent.unwrap_or(true);

        // Placeholder logic; wire to actual dbgeng/windbg or kd scripts if desired.
        let mut analyzed: Vec<Value> = Vec::new();
        if let Some(p) = dump_path {
            analyzed.push(json!({
                "file": p,
                "crash_code": "0x0000007E",
                "module": "ntoskrnl.exe",
                "analysis": "System service exception",
            }));
        }
        if include_recent {
            analyzed.push(json!({
                "file": "C:/Windows/Minidump/012125-1234-01.dmp",
                "crash_code": "0x0000003B",
                "module": "dxgmms2.sys",
                "analysis": "Graphics driver access violation",
            }));
        }
        let summary = format!("Analyzed {} dump file(s). Common patterns: driver issues, memory pressure.", analyzed.len());
        Ok(CallToolResult::success(vec![Content::json(json!({
            "summary": summary,
            "analysis": analyzed,
            "recommendations": [
                "Update GPU drivers",
                "Run memory diagnostics",
            ],
        })).map_err(to_internal)?]))
    }

    /// Parse Windows Event Logs and surface patterns.
    #[tool(name = "analyze_event_logs", description = "Parse and analyze Windows Event Viewer logs for patterns and issues.")]
    async fn analyze_event_logs(
        &self,
        Parameters(params): Parameters<AnalyzeEventLogsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let log_name = params.log_name.unwrap_or_else(|| "System".to_string());
        let hours = params.time_range.as_ref().map(|t| t.hours_back).unwrap_or(24);
        let _severity = params.severity; // Not used in mock
        
        // Placeholder: swap with Windows eventlog API ingestion.
        let events = vec![json!({
            "id": 41, "level": "Error", "source": "Kernel-Power", "message": "The system has rebooted without cleanly shutting down first.",
        })];
        let critical: Vec<_> = events.clone();
        let patterns = vec!["Repeated service failures".to_string()];
        let recs = vec!["Check service dependencies".to_string()];

        Ok(CallToolResult::success(vec![Content::json(json!({
            "log_name": log_name,
            "hours_analyzed": hours,
            "total_events": events.len(),
            "critical_events": critical,
            "error_patterns": patterns,
            "recommendations": recs,
        })).map_err(to_internal)?]))
    }

    /// Build a performance report over a time window.
    #[tool(name = "generate_performance_report", description = "Generate comprehensive system performance analysis reports.")]
    async fn generate_performance_report(
        &self,
        Parameters(p): Parameters<GeneratePerformanceReportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hours = p.duration_hours.unwrap_or(24);
        let include_processes = p.include_processes.unwrap_or(true);
        let include_hardware = p.include_hardware.unwrap_or(true);

        // Placeholder telemetry; attach to PDH/WMI/perf counters as needed.
        let cpu = json!({"average_usage": 35.2, "peak_usage": 78.9, "duration_hours": hours});
        let mem = json!({"average_usage_percent": 62.5, "peak_usage_percent": 89.1, "total_gb": 32});
        let disk = json!({"average_queue_length": 0.8, "peak_queue_length": 4.2});
        let net = json!({"average_bandwidth_mbps": 15.3, "peak_bandwidth_mbps": 85.7});
        let process = include_processes.then(|| json!({
            "top_cpu_processes": [
                {"name": "chrome.exe", "cpu_percent": 12.5},
                {"name": "System", "cpu_percent": 8.2}
            ]
        }));
        let hw = include_hardware.then(|| json!({"cpu_temp": 58, "gpu_temp": 65, "fan_speeds": {"cpu": 1850, "case": 1200}}));

        let recs = vec![
            "Consider adding more RAM".to_string(),
            "Schedule disk maintenance".to_string(),
        ];

        Ok(CallToolResult::success(vec![Content::json(json!({
            "summary": "System performance is within normal parameters",
            "cpu_analysis": cpu,
            "memory_analysis": mem,
            "disk_analysis": disk,
            "network_analysis": net,
            "process_analysis": process,
            "hardware_metrics": hw,
            "recommendations": recs,
        })).map_err(to_internal)?]))
    }

    /// Summarize system state and health.
    #[tool(name = "get_system_summary", description = "Generate overall system health and configuration summary.")]
    async fn get_system_summary(
        &self,
        Parameters(p): Parameters<GetSystemSummaryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let hw = p.include_hardware.unwrap_or(true);
        let sw = p.include_software.unwrap_or(true);
        let net = p.include_network.unwrap_or(true);

        Ok(CallToolResult::success(vec![Content::json(json!({
            "overview": "System is operating normally",
            "hardware_summary": if hw { Some("Intel i7, 32GB RAM, NVMe") } else { None },
            "software_summary": if sw { Some("Up‑to‑date; 127 programs installed") } else { None },
            "network_summary": if net { Some("Ethernet connected; DNS ok") } else { None },
            "health_score": 8.5,
            "critical_issues": [],
        })).map_err(to_internal)?]))
    }

    /// Provide intelligent shell completions.
    #[tool(name = "complete_command", description = "Provide intelligent command completions for various shells.")]
    async fn complete_command(
        &self,
        Parameters(p): Parameters<CompleteCommandParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let shell = p.shell_type.unwrap_or(ShellType::Cmd);
        let mut completions: Vec<Value> = Vec::new();
        match shell {
            ShellType::Cmd => {
                if p.partial_command.starts_with('d') {
                    completions.push(json!({"completion": "dir", "description": "List directory contents", "confidence": 0.95}));
                }
                if p.partial_command.starts_with('s') {
                    completions.push(json!({"completion": "systeminfo", "description": "Display system information", "confidence": 0.90}));
                }
            }
            ShellType::PowerShell => {
                if p.partial_command.to_lowercase().starts_with("get-") {
                    completions.push(json!({"completion": "Get-Process", "description": "Get running processes", "confidence": 0.95}));
                }
            }
            _ => {}
        }

        Ok(CallToolResult::success(vec![Content::json(json!({
            "partial_command": p.partial_command,
            "shell_type": format!("{:?}", shell),
            "completions": completions,
            "context": p.context,
        })).map_err(to_internal)?]))
    }

    /// Execute a script with an approval workflow.
    #[tool(name = "execute_script", description = "Execute diagnostic scripts with approval workflow.")]
    async fn execute_script(
        &self,
        Parameters(p): Parameters<ExecuteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let require_approval = p.require_approval.unwrap_or(true);
        let risk = assess_script_risk(&p.script);
        if require_approval {
            let approval_id = Uuid::new_v4().to_string();
            return Ok(CallToolResult::success(vec![Content::json(json!({
                "success": false,
                "approval_required": true,
                "approval_id": approval_id,
                "script_type": format!("{:?}", p.script_type),
                "description": p.description,
                "risk_level": format!("{:?}", risk),
                "message": "Script execution requires approval",
            })).map_err(to_internal)?]));
        }
        // Placeholder executor; wire to your sandboxed runner.
        Ok(CallToolResult::success(vec![Content::json(json!({
            "success": true,
            "output": "Script executed successfully",
            "error": Value::Null,
            "approval_required": false,
        })).map_err(to_internal)?]))
    }

    /// Simple wait tool useful for orchestration.
    #[tool(name = "wait", description = "Wait/sleep for the specified milliseconds (default 2000ms).")]
    async fn wait_tool(
        &self,
        Parameters(DurationParams { duration_ms }): Parameters<DurationParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ms = duration_ms.unwrap_or(2000);
        sleep(Duration::from_millis(ms)).await;
        Ok(CallToolResult::success(vec![Content::json(json!({
            "status": "success", "duration_ms": ms
        })).map_err(to_internal)?]))
    }
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DurationParams { #[schemars(default)] duration_ms: Option<u64> }

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

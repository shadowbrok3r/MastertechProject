use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Supported LLM providers for MCP integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmProvider {
    OpenAI {
        api_key: String,
        model: String,
    },
    Anthropic {
        api_key: String,
        model: String,
    },
    Local {
        endpoint: String,
        model: String,
    },
    Azure {
        endpoint: String,
        api_key: String,
        deployment: String,
    },
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::OpenAI {
            api_key: String::new(),
            model: "gpt-4".to_string(),
        }
    }
}

/// Diagnostic commands that can be executed via MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticCommand {
    /// Analyze Windows BSOD dump files
    AnalyzeBsod {
        dump_path: Option<PathBuf>,
        include_recent: bool,
    },
    /// Parse and analyze Windows Event Viewer logs
    AnalyzeEventLogs {
        log_name: String, // System, Application, Security
        time_range: Option<EventLogTimeRange>,
        severity: Option<EventLogSeverity>,
    },
    /// Generate system performance report
    GeneratePerformanceReport {
        duration_hours: u32,
        include_processes: bool,
        include_hardware: bool,
    },
    /// Get general system summary
    GetSystemSummary {
        include_hardware: bool,
        include_software: bool,
        include_network: bool,
    },
    /// Execute a command with approval required
    ExecuteScript {
        script: String,
        script_type: ScriptType,
        description: String,
        require_approval: bool,
    },
    /// Get command completion suggestions
    GetCommandCompletions {
        partial_command: String,
        shell_type: ShellType,
        context: Option<String>,
    },
}

/// Response from diagnostic operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticResponse {
    /// BSOD analysis results
    BsodAnalysis {
        summary: String,
        crash_reason: Option<String>,
        driver_issues: Vec<String>,
        recommendations: Vec<String>,
        dump_files_analyzed: Vec<PathBuf>,
    },
    /// Event log analysis results
    EventLogAnalysis {
        summary: String,
        critical_events: Vec<EventLogEntry>,
        error_patterns: Vec<String>,
        recommendations: Vec<String>,
        total_events_analyzed: u32,
    },
    /// Performance report
    PerformanceReport {
        summary: String,
        cpu_analysis: String,
        memory_analysis: String,
        disk_analysis: String,
        network_analysis: String,
        recommendations: Vec<String>,
        charts_data: Option<String>, // JSON data for charts
    },
    /// System summary
    SystemSummary {
        overview: String,
        hardware_summary: Option<String>,
        software_summary: Option<String>,
        network_summary: Option<String>,
        health_score: Option<f32>,
        critical_issues: Vec<String>,
    },
    /// Script execution result
    ScriptExecution {
        success: bool,
        output: String,
        error: Option<String>,
        approval_required: bool,
        approved: bool,
    },
    /// Command completions
    CommandCompletions {
        completions: Vec<CommandCompletion>,
        context_info: Option<String>,
    },
    /// Error response
    Error {
        message: String,
        details: Option<String>,
    },
}

/// Time range for event log analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogTimeRange {
    pub hours_back: u32,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// Event log severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventLogSeverity {
    Critical,
    Error,
    Warning,
    Information,
    Verbose,
}

/// Event log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogEntry {
    pub id: u32,
    pub level: EventLogSeverity,
    pub source: String,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub details: Option<String>,
}

/// Types of scripts that can be executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptType {
    PowerShell,
    Batch,
    Python,
    Bash,
}

/// Shell types for command completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShellType {
    Cmd,
    PowerShell,
    Bash,
    Zsh,
}

/// Command completion suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCompletion {
    pub completion: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub confidence: f32,
}

/// MCP tool definition for diagnostic operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub handler: String,
}

/// Request for script approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptApprovalRequest {
    pub id: String,
    pub script: String,
    pub script_type: ScriptType,
    pub description: String,
    pub ai_generated: bool,
    pub risk_level: RiskLevel,
    pub estimated_duration: Option<u32>, // seconds
}

/// Risk levels for script execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,    // Read-only operations, safe commands
    Medium, // System information gathering, non-destructive changes
    High,   // System modifications, registry changes
    Critical, // Potentially destructive operations
}

/// Approval response for script execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptApprovalResponse {
    pub request_id: String,
    pub approved: bool,
    pub reason: Option<String>,
    pub approved_by: String,
    pub approved_at: chrono::DateTime<chrono::Utc>,
}
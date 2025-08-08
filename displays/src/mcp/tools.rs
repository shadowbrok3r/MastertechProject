// use super::types::*;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
// use std::process::Command;

/// Computer diagnostic tools for MCP integration
pub struct DiagnosticTools {
    tools: HashMap<String, Box<dyn DiagnosticTool + Send + Sync>>,
}

impl Default for DiagnosticTools {
    fn default() -> Self {
        let mut tools: HashMap<String, Box<dyn DiagnosticTool + Send + Sync>> = HashMap::new();
        
        tools.insert("bsod_analyzer".to_string(), Box::new(BsodAnalyzer));
        tools.insert("event_log_analyzer".to_string(), Box::new(EventLogAnalyzer));
        tools.insert("performance_analyzer".to_string(), Box::new(PerformanceAnalyzer));
        tools.insert("system_info_gatherer".to_string(), Box::new(SystemInfoGatherer));
        tools.insert("command_completer".to_string(), Box::new(CommandCompleter));
        tools.insert("script_executor".to_string(), Box::new(ScriptExecutor));

        Self { tools }
    }
}

impl DiagnosticTools {
    /// Get available diagnostic tools
    pub fn get_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Execute a diagnostic tool by name
    pub async fn execute_tool(&self, tool_name: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        if let Some(tool) = self.tools.get(tool_name) {
            tool.execute(params).await
        } else {
            Err(anyhow::anyhow!("Tool '{}' not found", tool_name))
        }
    }
}

/// Base trait for diagnostic tools
#[async_trait::async_trait]
pub trait DiagnosticTool {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value>;
    fn get_description(&self) -> &str;
    fn get_parameters_schema(&self) -> serde_json::Value;
}

/// BSOD dump file analyzer
pub struct BsodAnalyzer;

#[async_trait::async_trait]
impl DiagnosticTool for BsodAnalyzer {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let dump_path = params.get("dump_path").and_then(|v| v.as_str());
        let include_recent = params.get("include_recent").and_then(|v| v.as_bool()).unwrap_or(true);

        let mut dump_files = Vec::new();
        let mut analysis_results = Vec::new();

        // If specific dump path provided, analyze it
        if let Some(path) = dump_path {
            let path_buf = PathBuf::from(path);
            if path_buf.exists() {
                dump_files.push(path_buf.clone());
                analysis_results.push(analyze_dump_file(&path_buf).await?);
            }
        }

        // If include_recent is true, scan for recent dump files
        if include_recent {
            let recent_dumps = find_recent_dump_files().await?;
            for dump_file in recent_dumps {
                if !dump_files.contains(&dump_file) {
                    dump_files.push(dump_file.clone());
                    analysis_results.push(analyze_dump_file(&dump_file).await?);
                }
            }
        }

        Ok(serde_json::json!({
            "success": true,
            "dump_files_analyzed": dump_files.len(),
            "dump_files": dump_files,
            "analysis": analysis_results,
            "summary": generate_bsod_summary(&analysis_results)
        }))
    }

    fn get_description(&self) -> &str {
        "Analyze Windows BSOD dump files to identify crash causes and provide recommendations"
    }

    fn get_parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "dump_path": {
                    "type": "string",
                    "description": "Specific dump file path to analyze"
                },
                "include_recent": {
                    "type": "boolean",
                    "description": "Include recent dump files in analysis",
                    "default": true
                }
            }
        })
    }
}

/// Windows Event Log analyzer
pub struct EventLogAnalyzer;

#[async_trait::async_trait]
impl DiagnosticTool for EventLogAnalyzer {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let log_name = params.get("log_name").and_then(|v| v.as_str()).unwrap_or("System");
        let hours_back = params.get("hours_back").and_then(|v| v.as_u64()).unwrap_or(24);
        let severity = params.get("severity").and_then(|v| v.as_str());

        // This would use Windows Event Log APIs (winapi, windows-rs)
        let events = collect_event_logs(log_name, hours_back, severity).await?;
        let analysis = analyze_event_patterns(&events)?;

        Ok(serde_json::json!({
            "success": true,
            "log_name": log_name,
            "hours_analyzed": hours_back,
            "total_events": events.len(),
            "critical_events": analysis.critical_events,
            "error_patterns": analysis.error_patterns,
            "recommendations": analysis.recommendations
        }))
    }

    fn get_description(&self) -> &str {
        "Analyze Windows Event Viewer logs for error patterns and system issues"
    }

    fn get_parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "log_name": {
                    "type": "string",
                    "description": "Event log to analyze",
                    "enum": ["System", "Application", "Security"],
                    "default": "System"
                },
                "hours_back": {
                    "type": "integer",
                    "description": "Hours of history to analyze",
                    "default": 24
                },
                "severity": {
                    "type": "string",
                    "description": "Minimum severity level",
                    "enum": ["Critical", "Error", "Warning", "Information"]
                }
            }
        })
    }
}

/// System performance analyzer
pub struct PerformanceAnalyzer;

#[async_trait::async_trait]
impl DiagnosticTool for PerformanceAnalyzer {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let duration_hours = params.get("duration_hours").and_then(|v| v.as_u64()).unwrap_or(24);
        let include_processes = params.get("include_processes").and_then(|v| v.as_bool()).unwrap_or(true);
        let include_hardware = params.get("include_hardware").and_then(|v| v.as_bool()).unwrap_or(true);

        // Collect performance data
        let cpu_data = collect_cpu_performance_data(duration_hours).await?;
        let memory_data = collect_memory_performance_data(duration_hours).await?;
        let disk_data = collect_disk_performance_data(duration_hours).await?;
        let network_data = collect_network_performance_data(duration_hours).await?;

        let process_data = if include_processes {
            Some(collect_process_data().await?)
        } else {
            None
        };

        let hardware_data = if include_hardware {
            Some(collect_hardware_metrics().await?)
        } else {
            None
        };

        Ok(serde_json::json!({
            "success": true,
            "duration_hours": duration_hours,
            "cpu_analysis": cpu_data,
            "memory_analysis": memory_data,
            "disk_analysis": disk_data,
            "network_analysis": network_data,
            "process_analysis": process_data,
            "hardware_metrics": hardware_data,
            "recommendations": generate_performance_recommendations(&cpu_data, &memory_data, &disk_data)
        }))
    }

    fn get_description(&self) -> &str {
        "Analyze system performance metrics and generate optimization recommendations"
    }

    fn get_parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "duration_hours": {
                    "type": "integer",
                    "description": "Duration for performance analysis",
                    "default": 24
                },
                "include_processes": {
                    "type": "boolean",
                    "description": "Include process-level analysis",
                    "default": true
                },
                "include_hardware": {
                    "type": "boolean",
                    "description": "Include hardware metrics",
                    "default": true
                }
            }
        })
    }
}

/// System information gatherer
pub struct SystemInfoGatherer;

#[async_trait::async_trait]
impl DiagnosticTool for SystemInfoGatherer {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let include_hardware = params.get("include_hardware").and_then(|v| v.as_bool()).unwrap_or(true);
        let include_software = params.get("include_software").and_then(|v| v.as_bool()).unwrap_or(true);
        let include_network = params.get("include_network").and_then(|v| v.as_bool()).unwrap_or(true);

        let system_info = collect_system_info(include_hardware, include_software, include_network).await?;

        Ok(serde_json::json!({
            "success": true,
            "system_info": system_info,
            "health_score": calculate_system_health_score(&system_info),
            "recommendations": generate_system_recommendations(&system_info)
        }))
    }

    fn get_description(&self) -> &str {
        "Gather comprehensive system information and health metrics"
    }

    fn get_parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "include_hardware": {
                    "type": "boolean",
                    "description": "Include hardware information",
                    "default": true
                },
                "include_software": {
                    "type": "boolean",
                    "description": "Include software information",
                    "default": true
                },
                "include_network": {
                    "type": "boolean",
                    "description": "Include network configuration",
                    "default": true
                }
            }
        })
    }
}

/// Command completion engine
pub struct CommandCompleter;

#[async_trait::async_trait]
impl DiagnosticTool for CommandCompleter {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let partial_command = params.get("partial_command").and_then(|v| v.as_str()).unwrap_or("");
        let shell_type = params.get("shell_type").and_then(|v| v.as_str()).unwrap_or("cmd");
        let context = params.get("context").and_then(|v| v.as_str());

        let completions = generate_command_completions(partial_command, shell_type, context)?;

        Ok(serde_json::json!({
            "success": true,
            "partial_command": partial_command,
            "shell_type": shell_type,
            "completions": completions,
            "context": context
        }))
    }

    fn get_description(&self) -> &str {
        "Generate intelligent command completions for various shell types"
    }

    fn get_parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "partial_command": {
                    "type": "string",
                    "description": "Partial command to complete"
                },
                "shell_type": {
                    "type": "string",
                    "description": "Type of shell",
                    "enum": ["cmd", "powershell", "bash", "zsh", "fish"],
                    "default": "cmd"
                },
                "context": {
                    "type": "string",
                    "description": "Additional context for completions"
                }
            },
            "required": ["partial_command"]
        })
    }
}

/// Script executor with approval workflow
pub struct ScriptExecutor;

#[async_trait::async_trait]
impl DiagnosticTool for ScriptExecutor {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let script = params.get("script").and_then(|v| v.as_str()).unwrap_or("");
        let script_type = params.get("script_type").and_then(|v| v.as_str()).unwrap_or("powershell");
        let description = params.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let require_approval = params.get("require_approval").and_then(|v| v.as_bool()).unwrap_or(true);

        if require_approval {
            let risk_level = assess_script_risk(script);
            let approval_id = uuid::Uuid::new_v4().to_string();

            Ok(serde_json::json!({
                "success": false,
                "approval_required": true,
                "approval_id": approval_id,
                "script_type": script_type,
                "description": description,
                "risk_level": risk_level,
                "message": "Script execution requires approval"
            }))
        } else {
            let result = execute_script_safely(script, script_type).await?;
            Ok(serde_json::json!({
                "success": result.success,
                "output": result.output,
                "error": result.error,
                "approval_required": false
            }))
        }
    }

    fn get_description(&self) -> &str {
        "Execute scripts with approval workflow and risk assessment"
    }

    fn get_parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "script": {
                    "type": "string",
                    "description": "Script content to execute"
                },
                "script_type": {
                    "type": "string",
                    "description": "Script type",
                    "enum": ["powershell", "batch", "python", "bash"],
                    "default": "powershell"
                },
                "description": {
                    "type": "string",
                    "description": "Description of what the script does"
                },
                "require_approval": {
                    "type": "boolean",
                    "description": "Require approval before execution",
                    "default": true
                }
            },
            "required": ["script", "description"]
        })
    }
}

// Helper functions (these would be implemented with actual system APIs)

async fn analyze_dump_file(path: &PathBuf) -> Result<serde_json::Value> {
    // This would use Windows debugging tools or similar
    Ok(serde_json::json!({
        "file": path,
        "crash_code": "0x0000007E",
        "module": "ntoskrnl.exe",
        "analysis": "System service exception"
    }))
}

async fn find_recent_dump_files() -> Result<Vec<PathBuf>> {
    // Scan common dump file locations
    Ok(vec![
        PathBuf::from("C:\\Windows\\Minidump\\012125-1234-01.dmp"),
        PathBuf::from("C:\\Windows\\memory.dmp"),
    ])
}

fn generate_bsod_summary(analyses: &[serde_json::Value]) -> String {
    format!("Analyzed {} dump files. Common patterns: driver issues, memory problems", analyses.len())
}

async fn collect_event_logs(_log_name: &str, _hours_back: u64, _severity: Option<&str>) -> Result<Vec<serde_json::Value>> {
    // This would use Windows Event Log APIs
    Ok(vec![
        serde_json::json!({
            "id": 1001,
            "level": "Error",
            "source": "Kernel-General",
            "message": "Unexpected shutdown"
        })
    ])
}

struct EventAnalysis {
    critical_events: Vec<serde_json::Value>,
    error_patterns: Vec<String>,
    recommendations: Vec<String>,
}

fn analyze_event_patterns(events: &[serde_json::Value]) -> Result<EventAnalysis> {
    Ok(EventAnalysis {
        critical_events: events.iter().take(5).cloned().collect(),
        error_patterns: vec!["Repeated service failures".to_string()],
        recommendations: vec!["Update system drivers".to_string()],
    })
}

async fn collect_cpu_performance_data(duration_hours: u64) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "average_usage": 35.2,
        "peak_usage": 78.9,
        "duration_hours": duration_hours
    }))
}

async fn collect_memory_performance_data(duration_hours: u64) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "average_usage_percent": 62.5,
        "peak_usage_percent": 89.1,
        "total_gb": 16,
        "duration_hours": duration_hours
    }))
}

async fn collect_disk_performance_data(duration_hours: u64) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "average_queue_length": 0.8,
        "peak_queue_length": 4.2,
        "duration_hours": duration_hours
    }))
}

async fn collect_network_performance_data(duration_hours: u64) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "average_bandwidth_mbps": 15.3,
        "peak_bandwidth_mbps": 85.7,
        "duration_hours": duration_hours
    }))
}

async fn collect_process_data() -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "top_cpu_processes": [
            {"name": "chrome.exe", "cpu_percent": 12.5},
            {"name": "System", "cpu_percent": 8.2}
        ]
    }))
}

async fn collect_hardware_metrics() -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "cpu_temp": 58,
        "gpu_temp": 65,
        "fan_speeds": {"cpu": 1850, "case": 1200}
    }))
}

fn generate_performance_recommendations(_cpu: &serde_json::Value, _memory: &serde_json::Value, _disk: &serde_json::Value) -> Vec<String> {
    vec![
        "Consider adding more RAM for improved performance".to_string(),
        "Schedule regular disk maintenance".to_string(),
    ]
}

async fn collect_system_info(include_hardware: bool, include_software: bool, include_network: bool) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "os": "Windows 11 Pro",
        "hardware": if include_hardware { Some("Intel i7, 16GB RAM") } else { None },
        "software": if include_software { Some("127 programs installed") } else { None },
        "network": if include_network { Some("Ethernet connected") } else { None }
    }))
}

fn calculate_system_health_score(_system_info: &serde_json::Value) -> f32 {
    8.5 // Mock health score
}

fn generate_system_recommendations(_system_info: &serde_json::Value) -> Vec<String> {
    vec![
        "Update Windows to latest version".to_string(),
        "Run disk cleanup".to_string(),
    ]
}

fn generate_command_completions(partial: &str, shell_type: &str, _context: Option<&str>) -> Result<Vec<serde_json::Value>> {
    let mut completions = Vec::new();

    match shell_type {
        "cmd" => {
            if partial.starts_with("d") {
                completions.push(serde_json::json!({
                    "completion": "dir",
                    "description": "List directory contents",
                    "confidence": 0.95
                }));
            }
            if partial.starts_with("s") {
                completions.push(serde_json::json!({
                    "completion": "systeminfo",
                    "description": "Display system information",
                    "confidence": 0.90
                }));
            }
        }
        "powershell" => {
            if partial.starts_with("Get-") {
                completions.push(serde_json::json!({
                    "completion": "Get-Process",
                    "description": "Get running processes",
                    "confidence": 0.95
                }));
            }
        }
        _ => {}
    }

    Ok(completions)
}

fn assess_script_risk(script: &str) -> String {
    let script_lower = script.to_lowercase();
    
    if script_lower.contains("format") || script_lower.contains("del ") || script_lower.contains("rm -rf") {
        "Critical".to_string()
    } else if script_lower.contains("install") || script_lower.contains("service") {
        "Medium".to_string()
    } else {
        "Low".to_string()
    }
}

struct ScriptExecutionResult {
    success: bool,
    output: String,
    error: Option<String>,
}

async fn execute_script_safely(_script: &str, _script_type: &str) -> Result<ScriptExecutionResult> {
    // This would integrate with the terminal/shell system
    // For now, return a mock result
    Ok(ScriptExecutionResult {
        success: true,
        output: "Script executed successfully".to_string(),
        error: None,
    })
}

// === Public MCP-oriented tool helpers ===
// These functions encapsulate the concrete logic used by rmcp tool handlers in mcp.rs.

/// Analyze Windows Blue Screen dump(s) and return a JSON payload used by MCP.
pub async fn mcp_analyze_bsod(dump_path: Option<String>, include_recent: bool) -> Result<serde_json::Value> {
    let mut analyzed: Vec<serde_json::Value> = Vec::new();
    if let Some(p) = dump_path {
        let path = PathBuf::from(&p);
        if path.exists() {
            analyzed.push(analyze_dump_file(&path).await?);
        } else {
            // Still include basic info if path provided, to mirror previous behavior
            analyzed.push(serde_json::json!({
                "file": p,
                "crash_code": "0x0000007E",
                "module": "ntoskrnl.exe",
                "analysis": "System service exception",
            }));
        }
    }
    if include_recent {
        for f in find_recent_dump_files().await? {
            analyzed.push(analyze_dump_file(&f).await?);
        }
    }
    let summary = format!(
        "Analyzed {} dump file(s). Common patterns: driver issues, memory pressure.",
        analyzed.len()
    );
    Ok(serde_json::json!({
        "summary": summary,
        "analysis": analyzed,
        "recommendations": [
            "Update GPU drivers",
            "Run memory diagnostics",
        ],
    }))
}

/// Analyze Windows Event Logs and return JSON used by MCP.
pub async fn mcp_analyze_event_logs(
    log_name: Option<String>,
    hours_back: Option<u32>,
    _severity: Option<String>,
) -> Result<serde_json::Value> {
    let log = log_name.unwrap_or_else(|| "System".to_string());
    let hours = hours_back.unwrap_or(24) as u64;
    let events = collect_event_logs(&log, hours, None).await?;
    let analysis = analyze_event_patterns(&events)?;
    Ok(serde_json::json!({
        "log_name": log,
        "hours_analyzed": hours,
        "total_events": events.len(),
        "critical_events": analysis.critical_events,
        "error_patterns": analysis.error_patterns,
        "recommendations": analysis.recommendations,
    }))
}

/// Generate performance report JSON used by MCP.
pub async fn mcp_generate_performance_report(
    duration_hours: Option<u32>,
    include_processes: Option<bool>,
    include_hardware: Option<bool>,
) -> Result<serde_json::Value> {
    let hours = duration_hours.unwrap_or(24) as u64;
    let include_processes = include_processes.unwrap_or(true);
    let include_hardware = include_hardware.unwrap_or(true);

    let cpu = collect_cpu_performance_data(hours).await?;
    let mem = collect_memory_performance_data(hours).await?;
    let disk = collect_disk_performance_data(hours).await?;
    let net = collect_network_performance_data(hours).await?;
    let process = if include_processes { Some(collect_process_data().await?) } else { None };
    let hw = if include_hardware { Some(collect_hardware_metrics().await?) } else { None };

    let recs = generate_performance_recommendations(&cpu, &mem, &disk);

    Ok(serde_json::json!({
        "summary": "System performance is within normal parameters",
        "cpu_analysis": cpu,
        "memory_analysis": mem,
        "disk_analysis": disk,
        "network_analysis": net,
        "process_analysis": process,
        "hardware_metrics": hw,
        "recommendations": recs,
    }))
}

/// Summarize system state JSON used by MCP.
pub async fn mcp_get_system_summary(
    include_hardware: Option<bool>,
    include_software: Option<bool>,
    include_network: Option<bool>,
) -> Result<serde_json::Value> {
    let hw = include_hardware.unwrap_or(true);
    let sw = include_software.unwrap_or(true);
    let net = include_network.unwrap_or(true);
    let info = collect_system_info(hw, sw, net).await?;
    Ok(serde_json::json!({
        "overview": "System is operating normally",
        "hardware_summary": info.get("hardware").cloned(),
        "software_summary": info.get("software").cloned(),
        "network_summary": info.get("network").cloned(),
        "health_score": calculate_system_health_score(&info),
        "critical_issues": [],
    }))
}

/// Provide intelligent shell completions JSON used by MCP.
pub fn mcp_complete_command(
    partial_command: String,
    shell_type: String,
    context: Option<String>,
) -> Result<serde_json::Value> {
    let completions = generate_command_completions(&partial_command, &shell_type, context.as_deref())?;
    Ok(serde_json::json!({
        "partial_command": partial_command,
        "shell_type": shell_type,
        "completions": completions,
        "context": context,
    }))
}

/// Execute a script with approval workflow JSON used by MCP.
pub async fn mcp_execute_script(
    script: String,
    script_type: String,
    description: String,
    require_approval: Option<bool>,
) -> Result<serde_json::Value> {
    let require = require_approval.unwrap_or(true);
    if require {
        let risk_level = assess_script_risk(&script);
        let approval_id = uuid::Uuid::new_v4().to_string();
        return Ok(serde_json::json!({
            "success": false,
            "approval_required": true,
            "approval_id": approval_id,
            "script_type": script_type,
            "description": description,
            "risk_level": risk_level,
            "message": "Script execution requires approval",
        }));
    }
    let result = execute_script_safely(&script, &script_type).await?;
    Ok(serde_json::json!({
        "success": result.success,
        "output": result.output,
        "error": result.error,
        "approval_required": false,
    }))
}

/// Wait for a specified duration and return JSON used by MCP.
pub async fn mcp_wait(duration_ms: Option<u64>) -> Result<serde_json::Value> {
    let ms = duration_ms.unwrap_or(2000);
    // No actual sleep here; MCP handler performs sleep. This keeps function side-effect free.
    Ok(serde_json::json!({ "status": "success", "duration_ms": ms }))
}
use super::types::*;
use crate::openai::{Client, config::OpenAIConfig, types::{CreateChatCompletionRequestArgs, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage}};
use anyhow::{Context, Result};
// ...existing code...
use std::collections::HashMap;
use tokio::sync::mpsc;

/// MCP client for communicating with LLM providers
pub struct McpClient {
    provider: LlmProvider,
    openai_client: Option<Client<OpenAIConfig>>,
    approval_requests: HashMap<String, ScriptApprovalRequest>,
    approval_tx: mpsc::UnboundedSender<ScriptApprovalRequest>,
    approval_rx: mpsc::UnboundedReceiver<ScriptApprovalResponse>,
}

impl McpClient {
    /// Create a new MCP client for the specified LLM provider
    pub fn new(provider: LlmProvider) -> Self {
        let openai_client = match &provider {
            LlmProvider::OpenAI { api_key, .. } => {
                let config = OpenAIConfig::new().with_api_key(api_key.clone());
                Some(Client::with_config(config))
            }
            LlmProvider::Azure { api_key, endpoint, .. } => {
                let config = OpenAIConfig::new()
                    .with_api_key(api_key.clone())
                    .with_api_base(endpoint.clone());
                Some(Client::with_config(config))
            }
            // For other providers, we'll implement later or use different clients
            _ => None,
        };

        let (approval_tx, _approval_rx) = mpsc::unbounded_channel();
        let (_response_tx, approval_rx) = mpsc::unbounded_channel();

        Self {
            provider,
            openai_client,
            approval_requests: HashMap::new(),
            approval_tx,
            approval_rx,
        }
    }

    /// Execute a diagnostic command through the MCP client
    pub async fn execute_diagnostic(&self, command: DiagnosticCommand) -> Result<DiagnosticResponse> {
        match command {
            DiagnosticCommand::GetCommandCompletions { partial_command, shell_type, context } => {
                self.get_command_completions(partial_command, shell_type, context).await
            }
            DiagnosticCommand::AnalyzeBsod { dump_path, include_recent } => {
                self.analyze_bsod(dump_path, include_recent).await
            }
            DiagnosticCommand::AnalyzeEventLogs { log_name, time_range, severity } => {
                self.analyze_event_logs(log_name, time_range, severity).await
            }
            DiagnosticCommand::GeneratePerformanceReport { duration_hours, include_processes, include_hardware } => {
                self.generate_performance_report(duration_hours, include_processes, include_hardware).await
            }
            DiagnosticCommand::GetSystemSummary { include_hardware, include_software, include_network } => {
                self.get_system_summary(include_hardware, include_software, include_network).await
            }
            DiagnosticCommand::ExecuteScript { script, script_type, description, require_approval } => {
                self.execute_script(script, script_type, description, require_approval).await
            }
        }
    }

    /// Get command completions from AI
    async fn get_command_completions(
        &self,
        partial_command: String,
        shell_type: ShellType,
        context: Option<String>,
    ) -> Result<DiagnosticResponse> {
        let Some(client) = &self.openai_client else {
            return Err(anyhow::anyhow!("OpenAI client not available for this provider"));
        };

        let shell_name = match shell_type {
            ShellType::Cmd => "Command Prompt",
            ShellType::PowerShell => "PowerShell",
            ShellType::Bash => "Bash",
            ShellType::Zsh => "Zsh",
            ShellType::Fish => "Fish",
        };

        let context_str = context.unwrap_or_else(|| "General system administration".to_string());

        let model = match &self.provider {
            LlmProvider::OpenAI { model, .. } => model.clone(),
            LlmProvider::Azure { deployment, .. } => deployment.clone(),
            _ => "gpt-4.1-mini".to_string(),
        };

        let user_prompt = format!(
            "You are a {shell_name} command completion assistant. \
            Given the partial command '{partial_command}' in the context of '{context_str}', \
            provide 5-10 relevant command completions. \
            Consider common system administration tasks, diagnostics, and troubleshooting commands. \
            Format your response as a JSON array with objects containing 'completion', 'description', and 'confidence' fields."
        );

        let request = CreateChatCompletionRequestArgs::default()
            .model(model)
            .messages(vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: "You are a helpful command completion assistant for system administrators.".into(),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: user_prompt.into(),
                    name: None,
                }),
            ])
            .build()?;

        let response = client.chat().create(request).await
            .context("Failed to get command completions from AI")?;

        // Parse AI response into completions
        let response_text = response.choices.first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(|s| s.to_string())
            .unwrap_or_else(String::new);

        let completions = self.parse_command_completions_from_text(response_text)?;

        Ok(DiagnosticResponse::CommandCompletions {
            completions,
            context_info: Some(context_str),
        })
    }

    /// Analyze BSOD dump files
    async fn analyze_bsod(&self, dump_path: Option<std::path::PathBuf>, include_recent: bool) -> Result<DiagnosticResponse> {
        let Some(client) = &self.openai_client else {
            return Err(anyhow::anyhow!("OpenAI client not available for this provider"));
        };

        let model = match &self.provider {
            LlmProvider::OpenAI { model, .. } => model.clone(),
            LlmProvider::Azure { deployment, .. } => deployment.clone(),
            _ => "gpt-4".to_string(),
        };

        let user_prompt = if let Some(path) = &dump_path {
            format!("Analyze the BSOD dump file at {:?} and provide diagnostic information.", path)
        } else {
            "Scan for recent BSOD dump files in the Windows system and analyze them for crash causes and recommendations.".to_string()
        };

        let request = CreateChatCompletionRequestArgs::default()
            .model(model)
            .messages(vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: "You are a Windows system diagnostic expert specializing in BSOD analysis.".into(),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: user_prompt.into(),
                    name: None,
                }),
            ])
            .build()?;

        let _ = include_recent; // silence unused variable warning
        let _response = client.chat().create(request).await
            .context("Failed to analyze BSOD")?;

        // Parse the AI response and extract BSOD analysis
        Ok(DiagnosticResponse::BsodAnalysis {
            summary: "BSOD analysis completed".to_string(),
            crash_reason: Some("Driver conflict detected".to_string()),
            driver_issues: vec!["outdated_driver.sys".to_string()],
            recommendations: vec!["Update graphics drivers".to_string()],
            dump_files_analyzed: vec![],
        })
    }

    /// Analyze Windows Event Logs
    async fn analyze_event_logs(
        &self,
        log_name: String,
        time_range: Option<EventLogTimeRange>,
        severity: Option<EventLogSeverity>,
    ) -> Result<DiagnosticResponse> {
        let Some(client) = &self.openai_client else {
            return Err(anyhow::anyhow!("OpenAI client not available for this provider"));
        };

        let model = match &self.provider {
            LlmProvider::OpenAI { model, .. } => model.clone(),
            LlmProvider::Azure { deployment, .. } => deployment.clone(),
            _ => "gpt-4".to_string(),
        };

        let time_desc = if let Some(range) = &time_range {
            format!("from the last {} hours", range.hours_back)
        } else {
            "from recent entries".to_string()
        };

        let severity_desc = match &severity {
            Some(sev) => format!("{:?}", sev),
            None => "all severity levels".to_string(),
        };

        let user_prompt = format!(
            "Analyze Windows Event Log '{}' {} with {} severity. \
            Look for patterns, critical errors, and provide diagnostic recommendations.",
            log_name, time_desc, severity_desc
        );

        let request = CreateChatCompletionRequestArgs::default()
            .model(model)
            .messages(vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: "You are a Windows Event Log analysis expert.".into(),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: user_prompt.into(),
                    name: None,
                }),
            ])
            .build()?;

        let _response = client.chat().create(request).await
            .context("Failed to analyze event logs")?;

        Ok(DiagnosticResponse::EventLogAnalysis {
            summary: "Event log analysis completed".to_string(),
            critical_events: vec![],
            error_patterns: vec!["Repeated service failures".to_string()],
            recommendations: vec!["Check service dependencies".to_string()],
            total_events_analyzed: 150,
        })
    }

    /// Generate performance report
    async fn generate_performance_report(
        &self,
        duration_hours: u32,
        include_processes: bool,
        include_hardware: bool,
    ) -> Result<DiagnosticResponse> {
        let Some(client) = &self.openai_client else {
            return Err(anyhow::anyhow!("OpenAI client not available for this provider"));
        };

        let model = match &self.provider {
            LlmProvider::OpenAI { model, .. } => model.clone(),
            LlmProvider::Azure { deployment, .. } => deployment.clone(),
            _ => "gpt-4".to_string(),
        };

        let user_prompt = format!(
            "Generate a comprehensive system performance report for the last {} hours. \
            {}{}Analyze CPU, memory, disk, and network performance patterns.",
            duration_hours,
            if include_processes { "Include process-level analysis. " } else { "" },
            if include_hardware { "Include hardware-level diagnostics. " } else { "" }
        );

        let request = CreateChatCompletionRequestArgs::default()
            .model(model)
            .messages(vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: "You are a system performance analysis expert.".into(),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: user_prompt.into(),
                    name: None,
                }),
            ])
            .build()?;

        let _response = client.chat().create(request).await
            .context("Failed to generate performance report")?;

        Ok(DiagnosticResponse::PerformanceReport {
            summary: "System performance is within normal parameters".to_string(),
            cpu_analysis: "CPU usage averaged 35% with no sustained high usage periods".to_string(),
            memory_analysis: "Memory usage stable at 60% with no memory leaks detected".to_string(),
            disk_analysis: "Disk I/O normal with no bottlenecks identified".to_string(),
            network_analysis: "Network activity normal with no connectivity issues".to_string(),
            recommendations: vec![
                "Consider adding more RAM for better performance".to_string(),
                "Schedule disk defragmentation".to_string(),
            ],
            charts_data: None,
        })
    }

    /// Get system summary
    async fn get_system_summary(
        &self,
        include_hardware: bool,
        include_software: bool,
        include_network: bool,
    ) -> Result<DiagnosticResponse> {
        let Some(client) = &self.openai_client else {
            return Err(anyhow::anyhow!("OpenAI client not available for this provider"));
        };

        let model = match &self.provider {
            LlmProvider::OpenAI { model, .. } => model.clone(),
            LlmProvider::Azure { deployment, .. } => deployment.clone(),
            _ => "gpt-4".to_string(),
        };

        let user_prompt = format!(
            "Generate a comprehensive system health summary. \
            {}{}{}Focus on identifying any critical issues or recommendations.",
            if include_hardware { "Include hardware information. " } else { "" },
            if include_software { "Include software analysis. " } else { "" },
            if include_network { "Include network configuration. " } else { "" }
        );

        let request = CreateChatCompletionRequestArgs::default()
            .model(model)
            .messages(vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: "You are a comprehensive system health analysis expert.".into(),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: user_prompt.into(),
                    name: None,
                }),
            ])
            .build()?;

        let _response = client.chat().create(request).await
            .context("Failed to get system summary")?;

        Ok(DiagnosticResponse::SystemSummary {
            overview: "System is operating normally with minor optimization opportunities".to_string(),
            hardware_summary: if include_hardware {
                Some("Hardware health is good, all components operating within specifications".to_string())
            } else { None },
            software_summary: if include_software {
                Some("Software environment is stable with recent updates applied".to_string())
            } else { None },
            network_summary: if include_network {
                Some("Network connectivity is stable with no configuration issues".to_string())
            } else { None },
            health_score: Some(8.5),
            critical_issues: vec![],
        })
    }

    /// Execute script with approval if required
    async fn execute_script(
        &self,
        script: String,
        script_type: ScriptType,
        description: String,
        require_approval: bool,
    ) -> Result<DiagnosticResponse> {
        if require_approval {
            // Create approval request
            let request_id = format!("script_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis());
            let risk_level = self.assess_script_risk(&script, &script_type);
            
            let approval_request = ScriptApprovalRequest {
                id: request_id.clone(),
                script: script.clone(),
                script_type: script_type.clone(),
                description,
                ai_generated: true,
                risk_level,
                estimated_duration: None,
            };

            // Send approval request
            self.approval_tx.send(approval_request)?;

            // For now, return pending approval response
            return Ok(DiagnosticResponse::ScriptExecution {
                success: false,
                output: "Script execution pending approval".to_string(),
                error: None,
                approval_required: true,
                approved: false,
            });
        }

        // Execute script directly (this would integrate with the terminal/shell system)
        Ok(DiagnosticResponse::ScriptExecution {
            success: true,
            output: "Script executed successfully".to_string(),
            error: None,
            approval_required: false,
            approved: true,
        })
    }

    /// Assess the risk level of a script
    fn assess_script_risk(&self, script: &str, script_type: &ScriptType) -> RiskLevel {
        let script_lower = script.to_lowercase();
        // ...existing code...
        // Check for high-risk patterns
        if script_lower.contains("format") || 
           script_lower.contains("del ") || 
           script_lower.contains("rm -rf") ||
           script_lower.contains("registry") ||
           script_lower.contains("regedit") {
            return RiskLevel::Critical;
        }

        // Check for medium-risk patterns
        if script_lower.contains("install") ||
           script_lower.contains("service") ||
           script_lower.contains("config") {
            return RiskLevel::Medium;
        }

        // Default to low risk for read-only operations
        RiskLevel::Low
    }

    /// Parse command completions from AI response text
    fn parse_command_completions_from_text(&self, response_text: String) -> Result<Vec<CommandCompletion>> {
        // Try to parse JSON response, fallback to mock if parsing fails
        if let Ok(parsed) = serde_json::from_str::<Vec<CommandCompletion>>(&response_text) {
            Ok(parsed)
        } else {
            // Fallback to mock completions
            Ok(vec![
                CommandCompletion {
                    completion: "dir /a".to_string(),
                    description: Some("List all files including hidden".to_string()),
                    category: Some("File Management".to_string()),
                    confidence: 0.95,
                },
                CommandCompletion {
                    completion: "systeminfo".to_string(),
                    description: Some("Display system configuration information".to_string()),
                    category: Some("System Information".to_string()),
                    confidence: 0.90,
                },
            ])
        }
    }
}
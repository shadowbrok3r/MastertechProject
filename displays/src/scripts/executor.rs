//! Script execution trait and utilities

use super::{ScriptCategory, ScriptChannels, ScriptItem, ScriptLogEntry, ScriptStatus};
use async_trait::async_trait;

/// Result of script execution
#[derive(Debug, Clone)]
pub enum ScriptResult {
    Success(String),
    Warning(String),
    Error(String),
    Skipped(String),
}

impl ScriptResult {
    pub fn is_success(&self) -> bool {
        matches!(self, ScriptResult::Success(_))
    }

    pub fn message(&self) -> &str {
        match self {
            ScriptResult::Success(msg) => msg,
            ScriptResult::Warning(msg) => msg,
            ScriptResult::Error(msg) => msg,
            ScriptResult::Skipped(msg) => msg,
        }
    }
}

/// Context passed to script executors
#[derive(Clone)]
pub struct ScriptContext {
    pub service_number: Option<String>,
    pub customer_email: Option<String>,
    pub channels: ScriptChannels,
}

impl Default for ScriptContext {
    fn default() -> Self {
        Self {
            service_number: None,
            customer_email: None,
            channels: ScriptChannels::default(),
        }
    }
}

impl ScriptContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_service_number(mut self, sn: impl Into<String>) -> Self {
        self.service_number = Some(sn.into());
        self
    }

    pub fn with_customer_email(mut self, email: impl Into<String>) -> Self {
        self.customer_email = Some(email.into());
        self
    }

    pub fn log(&self, entry: ScriptLogEntry) {
        let _ = self.channels.log_tx.try_send(entry);
    }

    pub fn log_info(&self, category: ScriptCategory, script_name: &str, message: impl Into<String>) {
        self.log(ScriptLogEntry::info(category, script_name, message));
    }

    pub fn log_success(&self, category: ScriptCategory, script_name: &str, message: impl Into<String>) {
        self.log(ScriptLogEntry::success(category, script_name, message));
    }

    pub fn log_warning(&self, category: ScriptCategory, script_name: &str, message: impl Into<String>) {
        self.log(ScriptLogEntry::warning(category, script_name, message));
    }

    pub fn log_error(&self, category: ScriptCategory, script_name: &str, message: impl Into<String>) {
        self.log(ScriptLogEntry::error(category, script_name, message));
    }

    pub fn report_progress(&self, script_id: &str, current: u64, total: u64) {
        let _ = self.channels.progress_tx.try_send((script_id.to_string(), current, total));
    }
}

/// Trait for script execution - implement per platform
#[async_trait]
pub trait ScriptExecutor: Send + Sync {
    /// Execute the script and return the result
    async fn execute(&self, script: &ScriptItem, ctx: &ScriptContext) -> ScriptResult;
    
    /// Get the script name this executor handles
    fn handles(&self) -> &'static str;
}

/// Registry of script executors
pub struct ScriptExecutorRegistry {
    executors: Vec<Box<dyn ScriptExecutor>>,
}

impl Default for ScriptExecutorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptExecutorRegistry {
    pub fn new() -> Self {
        Self {
            executors: Vec::new(),
        }
    }

    pub fn register(&mut self, executor: Box<dyn ScriptExecutor>) {
        self.executors.push(executor);
    }

    pub fn find_executor(&self, script_name: &str) -> Option<&dyn ScriptExecutor> {
        self.executors
            .iter()
            .find(|e| e.handles() == script_name)
            .map(|e| e.as_ref())
    }

    pub async fn execute(&self, script: &ScriptItem, ctx: &ScriptContext) -> ScriptResult {
        if let Some(executor) = self.find_executor(&script.name) {
            executor.execute(script, ctx).await
        } else {
            ScriptResult::Error(format!("No executor found for script: {}", script.name))
        }
    }
}

/// Helper to update script status after execution
pub fn update_script_status(script: &mut ScriptItem, result: &ScriptResult) {
    script.status = match result {
        ScriptResult::Success(_) => ScriptStatus::Completed,
        ScriptResult::Warning(_) => ScriptStatus::Completed,
        ScriptResult::Error(_) => ScriptStatus::Failed,
        ScriptResult::Skipped(_) => ScriptStatus::Skipped,
    };
}


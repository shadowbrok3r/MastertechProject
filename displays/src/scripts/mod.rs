//! Shared scripts module for both egui and terminal mode
//! 
//! This module contains the core script definitions, categories, and execution
//! traits that can be used by both the egui GUI and terminal mode interfaces.

use serde::{Deserialize, Serialize};
use std::fmt::Display;
use crossbeam::channel::{Receiver, Sender};

pub mod categories;
pub mod executor;
pub mod mcp_channel;
pub mod queue;

pub use categories::*;
pub use executor::*;
pub use mcp_channel::*;
pub use queue::*;

/// Script execution status
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptStatus {
    #[default]
    Pending,
    Selected,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl Display for ScriptStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptStatus::Pending => write!(f, "Pending"),
            ScriptStatus::Selected => write!(f, "Selected"),
            ScriptStatus::Running => write!(f, "Running"),
            ScriptStatus::Completed => write!(f, "Completed"),
            ScriptStatus::Failed => write!(f, "Failed"),
            ScriptStatus::Skipped => write!(f, "Skipped"),
        }
    }
}

/// Planned wall-clock budget in seconds for one remote script run.
pub fn default_remote_script_timeout_secs(script_name: &str) -> u64 {
    match script_name {
        "Run Tron" | "Data Transfer" => 7200,
        "Install Windows Updates" | "Run SuperAntiSpyware Scan" | "Run Webroot Scan" => 3600,
        "Activate CPS" | "Activate Webroot" | "Activate SuperAnti" | "Activate SEB" => 1800,
        // 12+ benchmarks at ~15 s each plus warmup and persistence.
        "Benchmark Suite" => 1800,
        "QC Benchmark" | "Memory Test" => 1200,
        "GPU Stress Test" | "Stress: PSU" | "Stress: Linpack" => 900,
        _ => 600,
    }
}

/// Script category types
#[derive(Default, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScriptCategory {
    #[default]
    Tuneup,
    Informational,
    JunkwareRemoval,
    StressTests,
    UserScripts(String),
    Custom(String),
}

impl Display for ScriptCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptCategory::Tuneup => write!(f, "Tuneup / QC"),
            ScriptCategory::Informational => write!(f, "Informational"),
            ScriptCategory::JunkwareRemoval => write!(f, "Junkware Removal"),
            ScriptCategory::StressTests => write!(f, "Stress Tests"),
            ScriptCategory::UserScripts(name) => write!(f, "User: {}", name),
            ScriptCategory::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Represents a single script item
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptItem {
    pub id: String,
    pub name: String,
    pub category: ScriptCategory,
    pub status: ScriptStatus,
    pub description: String,
    pub pass_criteria: Option<String>,
    pub warning_criteria: Option<String>,
    pub error_criteria: Option<String>,
}

impl Default for ScriptItem {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            category: ScriptCategory::default(),
            status: ScriptStatus::default(),
            description: String::new(),
            pass_criteria: None,
            warning_criteria: None,
            error_criteria: None,
        }
    }
}

impl ScriptItem {
    pub fn new(name: impl Into<String>, category: ScriptCategory) -> Self {
        let name = name.into();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.clone(),
            category,
            status: ScriptStatus::Pending,
            description: String::new(),
            pass_criteria: None,
            warning_criteria: None,
            error_criteria: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_pass_criteria(mut self, criteria: impl Into<String>) -> Self {
        self.pass_criteria = Some(criteria.into());
        self
    }

    pub fn with_warning_criteria(mut self, criteria: impl Into<String>) -> Self {
        self.warning_criteria = Some(criteria.into());
        self
    }

    pub fn with_error_criteria(mut self, criteria: impl Into<String>) -> Self {
        self.error_criteria = Some(criteria.into());
        self
    }

    pub fn is_selected(&self) -> bool {
        self.status == ScriptStatus::Selected
    }

    pub fn toggle_selection(&mut self) {
        self.status = match self.status {
            ScriptStatus::Pending => ScriptStatus::Selected,
            ScriptStatus::Selected => ScriptStatus::Pending,
            other => other,
        };
    }

    pub fn select(&mut self) {
        if self.status == ScriptStatus::Pending {
            self.status = ScriptStatus::Selected;
        }
    }

    pub fn deselect(&mut self) {
        if self.status == ScriptStatus::Selected {
            self.status = ScriptStatus::Pending;
        }
    }
}

/// A log entry from script execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub category: ScriptCategory,
    pub script_name: String,
    pub message: String,
    pub level: LogLevel,
}

impl ScriptLogEntry {
    pub fn new(category: ScriptCategory, script_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            category,
            script_name: script_name.into(),
            message: message.into(),
            level: LogLevel::Info,
        }
    }

    pub fn info(category: ScriptCategory, script_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(category, script_name, message)
    }

    pub fn success(category: ScriptCategory, script_name: impl Into<String>, message: impl Into<String>) -> Self {
        let mut entry = Self::new(category, script_name, message);
        entry.level = LogLevel::Success;
        entry
    }

    pub fn warning(category: ScriptCategory, script_name: impl Into<String>, message: impl Into<String>) -> Self {
        let mut entry = Self::new(category, script_name, message);
        entry.level = LogLevel::Warning;
        entry
    }

    pub fn error(category: ScriptCategory, script_name: impl Into<String>, message: impl Into<String>) -> Self {
        let mut entry = Self::new(category, script_name, message);
        entry.level = LogLevel::Error;
        entry
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Channels for script communication
pub struct ScriptChannels {
    pub log_tx: Sender<ScriptLogEntry>,
    pub log_rx: Receiver<ScriptLogEntry>,
    pub progress_tx: Sender<(String, u64, u64)>, // (script_id, current, total)
    pub progress_rx: Receiver<(String, u64, u64)>,
}

impl Default for ScriptChannels {
    fn default() -> Self {
        let (log_tx, log_rx) = crossbeam::channel::unbounded();
        let (progress_tx, progress_rx) = crossbeam::channel::unbounded();
        Self {
            log_tx,
            log_rx,
            progress_tx,
            progress_rx,
        }
    }
}

impl Clone for ScriptChannels {
    fn clone(&self) -> Self {
        Self {
            log_tx: self.log_tx.clone(),
            log_rx: self.log_rx.clone(),
            progress_tx: self.progress_tx.clone(),
            progress_rx: self.progress_rx.clone(),
        }
    }
}


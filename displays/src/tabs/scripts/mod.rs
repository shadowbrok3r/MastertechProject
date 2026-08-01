//! Scripts tab for egui with categories, queue management, and log view
//! 
//! Layout:
//! - Left panel: Categories with collapsible checkboxes
//! - Middle panel: Script queue with drag-and-drop reordering
//! - Right panel: Scrollable log view

mod ui;

use crate::scripts::{
    ScriptCategory, ScriptChannels, ScriptContext, ScriptLogEntry, ScriptsState,
};

/// The scripts tab component for egui
pub struct ScriptsTab {
    /// State containing categories, queue, and logs
    pub state: ScriptsState,
    /// Communication channels
    pub channels: ScriptChannels,
    /// Service number input
    pub service_number_input: String,
    /// Whether the log should auto-scroll
    pub auto_scroll_logs: bool,
}

impl Default for ScriptsTab {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptsTab {
    pub fn new() -> Self {
        Self {
            state: ScriptsState::new(),
            channels: ScriptChannels::default(),
            service_number_input: String::new(),
            auto_scroll_logs: true,
        }
    }

    /// Process incoming messages from channels
    pub fn receive(&mut self) {
        // Receive log messages
        while let Ok(log_entry) = self.channels.log_rx.try_recv() {
            self.state.logs.push(log_entry);
        }

        // Receive progress updates
        while let Ok((script_id, current, total)) = self.channels.progress_rx.try_recv() {
            self.state.current_progress = Some((current, total));
            // Find the script and update if needed
            if let Some(qs) = self.state.queue.items().iter().find(|qs| qs.script.id == script_id) {
                self.state.current_script_name = Some(qs.script.name.clone());
            }
            // Reset progress when complete
            if current >= total {
                self.state.current_progress = None;
            }
        }
    }

    /// Get the script context for execution
    pub fn get_context(&self) -> ScriptContext {
        ScriptContext {
            service_number: if self.service_number_input.is_empty() { 
                None 
            } else { 
                Some(self.service_number_input.clone()) 
            },
            customer_email: None,
            channels: self.channels.clone(),
        }
    }

    /// Add selected scripts to the queue
    pub fn queue_selected_scripts(&mut self) {
        let selected = self.state.get_selected_scripts();
        if selected.is_empty() {
            self.state.log(ScriptLogEntry::warning(
                ScriptCategory::Custom("System".to_string()),
                "Queue",
                "No scripts selected to queue",
            ));
            return;
        }
        
        let count = selected.len();
        self.state.queue.add_all(selected);
        self.state.clear_selections();
        
        self.state.log(ScriptLogEntry::info(
            ScriptCategory::Custom("System".to_string()),
            "Queue",
            format!("Added {} scripts to queue", count),
        ));
    }

    /// Clear the script queue
    pub fn clear_queue(&mut self) {
        self.state.queue.clear();
        self.state.log(ScriptLogEntry::info(
            ScriptCategory::Custom("System".to_string()),
            "Queue",
            "Queue cleared",
        ));
    }

    /// Run all scripts in the queue
    pub fn run_queue(&mut self) {
        if self.state.queue.is_empty() {
            self.state.log(ScriptLogEntry::warning(
                ScriptCategory::Custom("System".to_string()),
                "Queue",
                "Queue is empty, nothing to run",
            ));
            return;
        }

        self.state.queue.start();
        self.state.log(ScriptLogEntry::info(
            ScriptCategory::Custom("System".to_string()),
            "Queue",
            format!("Starting queue execution ({} scripts)", self.state.queue.len()),
        ));
        
        // The actual script execution will be handled by the platform-specific code
        // This just starts the queue state machine
    }

    /// Stop queue execution
    pub fn stop_queue(&mut self) {
        self.state.queue.stop();
        self.state.log(ScriptLogEntry::warning(
            ScriptCategory::Custom("System".to_string()),
            "Queue",
            "Queue execution stopped",
        ));
    }
}


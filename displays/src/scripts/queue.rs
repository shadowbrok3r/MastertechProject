//! Script queue management for ordering and executing scripts

use super::{ScriptCategory, ScriptItem, ScriptLogEntry, ScriptStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A queued script with its execution order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedScript {
    pub order: usize,
    pub script: ScriptItem,
}

impl std::hash::Hash for QueuedScript {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.script.id.hash(state);
        self.order.hash(state);
    }
}

impl QueuedScript {
    pub fn new(order: usize, script: ScriptItem) -> Self {
        Self { order, script }
    }
}

/// Manages the script execution queue with drag-and-drop reordering
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptQueue {
    /// Scripts in execution order
    items: Vec<QueuedScript>,
    /// Currently running script index
    current_index: Option<usize>,
    /// Is the queue currently running
    is_running: bool,
}

impl ScriptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a script to the queue
    pub fn add(&mut self, script: ScriptItem) {
        let order = self.items.len();
        self.items.push(QueuedScript::new(order, script));
        self.renumber();
    }

    /// Add multiple scripts to the queue
    pub fn add_all(&mut self, scripts: Vec<ScriptItem>) {
        for script in scripts {
            self.add(script);
        }
    }

    /// Remove a script from the queue by id
    pub fn remove(&mut self, script_id: &str) {
        self.items.retain(|qs| qs.script.id != script_id);
        self.renumber();
    }

    /// Clear all scripts from the queue
    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = None;
        self.is_running = false;
    }

    /// Move a script from one position to another
    pub fn move_item(&mut self, from_index: usize, to_index: usize) {
        if from_index < self.items.len() && to_index < self.items.len() {
            let item = self.items.remove(from_index);
            self.items.insert(to_index, item);
            self.renumber();
        }
    }

    /// Renumber all items after a change
    fn renumber(&mut self) {
        for (i, item) in self.items.iter_mut().enumerate() {
            item.order = i;
        }
    }

    /// Get all queued scripts
    pub fn items(&self) -> &[QueuedScript] {
        &self.items
    }

    /// Get mutable access to items (for drag-and-drop)
    pub fn items_mut(&mut self) -> &mut Vec<QueuedScript> {
        &mut self.items
    }

    /// Get the number of scripts in the queue
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Check if the queue is currently running
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Get the currently running script
    pub fn current_script(&self) -> Option<&QueuedScript> {
        self.current_index.and_then(|i| self.items.get(i))
    }

    /// Start running the queue
    pub fn start(&mut self) {
        if !self.items.is_empty() {
            self.is_running = true;
            self.current_index = Some(0);
            if let Some(item) = self.items.get_mut(0) {
                item.script.status = ScriptStatus::Running;
            }
        }
    }

    /// Mark current script as complete and move to next
    pub fn next(&mut self) -> Option<&QueuedScript> {
        if let Some(current) = self.current_index {
            // Mark current as completed if still running
            if let Some(item) = self.items.get_mut(current) {
                if item.script.status == ScriptStatus::Running {
                    item.script.status = ScriptStatus::Completed;
                }
            }

            // Move to next
            let next_index = current + 1;
            if next_index < self.items.len() {
                self.current_index = Some(next_index);
                if let Some(item) = self.items.get_mut(next_index) {
                    item.script.status = ScriptStatus::Running;
                }
                return self.items.get(next_index);
            } else {
                // Queue complete
                self.is_running = false;
                self.current_index = None;
            }
        }
        None
    }

    /// Stop the queue execution
    pub fn stop(&mut self) {
        self.is_running = false;
        // Reset running script to selected
        if let Some(current) = self.current_index {
            if let Some(item) = self.items.get_mut(current) {
                if item.script.status == ScriptStatus::Running {
                    item.script.status = ScriptStatus::Selected;
                }
            }
        }
        self.current_index = None;
    }

    /// Get progress as (completed, total)
    pub fn progress(&self) -> (usize, usize) {
        let completed = self.items.iter()
            .filter(|qs| matches!(qs.script.status, ScriptStatus::Completed | ScriptStatus::Failed | ScriptStatus::Skipped))
            .count();
        (completed, self.items.len())
    }
}

/// State manager for the scripts UI
#[derive(Debug, Clone, Default)]
pub struct ScriptsState {
    /// All available scripts organized by category
    pub categories: HashMap<ScriptCategory, Vec<ScriptItem>>,
    /// The execution queue
    pub queue: ScriptQueue,
    /// Log entries
    pub logs: Vec<ScriptLogEntry>,
    /// Category expansion state (for collapsible headers)
    pub category_expanded: HashMap<ScriptCategory, bool>,
    /// Service number input
    pub service_number: String,
    /// Current progress for active script (current, total)
    pub current_progress: Option<(u64, u64)>,
    /// Currently running script name
    pub current_script_name: Option<String>,
}

impl ScriptsState {
    pub fn new() -> Self {
        let mut state = Self::default();
        state.categories = super::get_all_categories();
        // Expand all categories by default
        for category in super::CATEGORY_ORDER.iter() {
            state.category_expanded.insert(category.clone(), true);
        }
        state
    }

    /// Get selected scripts from all categories
    pub fn get_selected_scripts(&self) -> Vec<ScriptItem> {
        self.categories
            .values()
            .flat_map(|scripts| scripts.iter().filter(|s| s.is_selected()).cloned())
            .collect()
    }

    /// Add all selected scripts to the queue
    pub fn queue_selected(&mut self) {
        let selected = self.get_selected_scripts();
        self.queue.add_all(selected);
    }

    /// Clear all selections
    pub fn clear_selections(&mut self) {
        for scripts in self.categories.values_mut() {
            for script in scripts.iter_mut() {
                script.deselect();
            }
        }
    }

    /// Select all scripts in a category
    pub fn select_category(&mut self, category: &ScriptCategory) {
        if let Some(scripts) = self.categories.get_mut(category) {
            for script in scripts.iter_mut() {
                script.select();
            }
        }
    }

    /// Deselect all scripts in a category
    pub fn deselect_category(&mut self, category: &ScriptCategory) {
        if let Some(scripts) = self.categories.get_mut(category) {
            for script in scripts.iter_mut() {
                script.deselect();
            }
        }
    }

    /// Toggle all scripts in a category
    pub fn toggle_category(&mut self, category: &ScriptCategory) {
        if let Some(scripts) = self.categories.get_mut(category) {
            let any_selected = scripts.iter().any(|s| s.is_selected());
            for script in scripts.iter_mut() {
                if any_selected {
                    script.deselect();
                } else {
                    script.select();
                }
            }
        }
    }

    /// Toggle script selection by id
    pub fn toggle_script(&mut self, script_id: &str) {
        for scripts in self.categories.values_mut() {
            if let Some(script) = scripts.iter_mut().find(|s| s.id == script_id) {
                script.toggle_selection();
                break;
            }
        }
    }

    /// Add a log entry
    pub fn log(&mut self, entry: ScriptLogEntry) {
        self.logs.push(entry);
    }

    /// Clear logs
    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    /// Get log entries for display (most recent last)
    pub fn logs(&self) -> &[ScriptLogEntry] {
        &self.logs
    }
}


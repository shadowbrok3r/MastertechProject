//! Script queue management for ordering and executing scripts

use super::{ScriptCategory, ScriptItem, ScriptLogEntry, ScriptStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A queued script with its execution order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedScript {
    pub order: usize,
    /// Identifies this entry for its whole life. The script's own id names which
    /// script it is, so two copies of one script would be indistinguishable by it
    /// and removing one would remove both.
    #[serde(default)]
    pub run_token: u64,
    pub script: ScriptItem,
}

impl std::hash::Hash for QueuedScript {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.run_token.hash(state);
        self.order.hash(state);
    }
}

impl QueuedScript {
    pub fn new(order: usize, run_token: u64, script: ScriptItem) -> Self {
        Self {
            order,
            run_token,
            script,
        }
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
    /// Source of `QueuedScript::run_token`; monotonic for the queue's lifetime.
    #[serde(default)]
    next_run_token: u64,
}

impl ScriptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a script to the queue
    pub fn add(&mut self, script: ScriptItem) {
        let order = self.items.len();
        self.next_run_token += 1;
        self.items
            .push(QueuedScript::new(order, self.next_run_token, script));
        self.renumber();
    }

    /// Add multiple scripts to the queue
    pub fn add_all(&mut self, scripts: Vec<ScriptItem>) {
        for script in scripts {
            self.add(script);
        }
    }

    /// Remove one queue entry. Keyed on the entry, not the script, so removing
    /// one of two copies of the same script leaves the other alone.
    pub fn remove(&mut self, run_token: u64) {
        self.items.retain(|qs| qs.run_token != run_token);
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

    /// Mark the currently running script with a terminal status.
    pub fn finish_current(&mut self, failed: bool) {
        if let Some(current) = self.current_index {
            if let Some(item) = self.items.get_mut(current) {
                if item.script.status == ScriptStatus::Running {
                    item.script.status = if failed {
                        ScriptStatus::Failed
                    } else {
                        ScriptStatus::Completed
                    };
                }
            }
        }
    }

    /// Stop the queue execution
    pub fn stop(&mut self) {
        self.is_running = false;
        // Reset running script to selected
        if let Some(current) = self.current_index {
            if let Some(item) = self.items.get_mut(current) {
                if item.script.status == ScriptStatus::Running {
                    item.script.status = ScriptStatus::Pending;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::ScriptCategory;

    fn queue_of(n: usize) -> ScriptQueue {
        let mut q = ScriptQueue::new();
        for i in 0..n {
            q.add(ScriptItem::new(format!("script {i}"), ScriptCategory::Tuneup));
        }
        q
    }

    #[test]
    fn drives_every_script_to_completion() {
        let mut q = queue_of(3);
        q.start();
        assert!(q.is_running());
        assert_eq!(q.current_script().unwrap().script.status, ScriptStatus::Running);

        // Script 0 succeeds, 1 fails, 2 succeeds.
        q.finish_current(false);
        assert_eq!(q.items()[0].script.status, ScriptStatus::Completed);
        assert!(q.next().is_some());

        q.finish_current(true);
        assert_eq!(q.items()[1].script.status, ScriptStatus::Failed);
        assert!(q.next().is_some());

        q.finish_current(false);
        assert_eq!(q.items()[2].script.status, ScriptStatus::Completed);
        assert!(q.next().is_none());

        assert!(!q.is_running());
        assert!(q.current_script().is_none());
        assert_eq!(q.progress(), (3, 3));
    }

    #[test]
    fn finish_current_only_touches_running_script() {
        let mut q = queue_of(1);
        q.start();
        q.finish_current(true);
        q.finish_current(false); // status no longer Running; must not flip to Completed
        assert_eq!(q.items()[0].script.status, ScriptStatus::Failed);
    }
}

/// Most log entries kept once trimming runs.
pub const LOG_CAP: usize = 5_000;
/// Overflow allowed before trimming, so the front is drained in batches rather than per push.
const LOG_TRIM_BATCH: usize = 500;

/// State manager for the scripts UI
#[derive(Debug, Clone, Default)]
pub struct ScriptsState {
    /// All available scripts organized by category
    pub categories: HashMap<ScriptCategory, Vec<ScriptItem>>,
    /// The execution queue
    pub queue: ScriptQueue,
    /// Log entries, oldest first; read through [`Self::logs`].
    logs: Vec<ScriptLogEntry>,
    /// Entries dropped from the front, so a cursor taken earlier still names the same entry.
    log_base: usize,
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
        state.categories = super::catalog::CATALOG.items_for(super::catalog::Surface::Egui);
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
        if self.logs.len() > LOG_CAP + LOG_TRIM_BATCH {
            let excess = self.logs.len() - LOG_CAP;
            self.logs.drain(..excess);
            self.log_base += excess;
        }
    }

    pub fn clear_logs(&mut self) {
        self.log_base += self.logs.len();
        self.logs.clear();
    }

    /// Log entries still held, oldest first.
    pub fn logs(&self) -> &[ScriptLogEntry] {
        &self.logs
    }

    /// Position just past the newest entry. Unlike a `Vec` index it survives trimming and clearing.
    pub fn log_cursor(&self) -> usize {
        self.log_base + self.logs.len()
    }

    /// Entries between two cursors; any already dropped are skipped.
    pub fn logs_between(&self, start: usize, end: usize) -> &[ScriptLogEntry] {
        let lo = start.saturating_sub(self.log_base).min(self.logs.len());
        let hi = end.saturating_sub(self.log_base).min(self.logs.len());
        &self.logs[lo..hi.max(lo)]
    }

    /// Entries at or after `start`.
    pub fn logs_since(&self, start: usize) -> &[ScriptLogEntry] {
        self.logs_between(start, self.log_cursor())
    }
}


#[cfg(test)]
mod queue_identity_tests {
    use super::*;
    use crate::scripts::ScriptCategory;

    fn item(name: &str) -> ScriptItem {
        ScriptItem::new(name, ScriptCategory::Tuneup)
    }

    /// Queueing the same script twice is legitimate, and removing one copy must
    /// leave the other. Keyed on the script instead of the entry, both would go.
    #[test]
    fn removing_one_copy_leaves_the_other() {
        let mut queue = ScriptQueue::new();
        queue.add(item("Run Webroot Scan"));
        queue.add(item("Run Webroot Scan"));
        assert_eq!(queue.len(), 2);

        let first = queue.items()[0].run_token;
        let second = queue.items()[1].run_token;
        assert_ne!(first, second, "two entries must not share an identity");

        queue.remove(first);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.items()[0].run_token, second);
        assert_eq!(queue.items()[0].order, 0, "the survivor must be renumbered");
    }

    /// A run token is never handed out twice, even after the entry that held it
    /// has been removed.
    #[test]
    fn run_tokens_are_not_reused() {
        let mut queue = ScriptQueue::new();
        queue.add(item("a"));
        let first = queue.items()[0].run_token;
        queue.remove(first);
        queue.add(item("b"));
        assert_ne!(queue.items()[0].run_token, first);
    }

    /// Selection used to be a status, so `select` refused to promote anything that
    /// had already run: a tech could not re-tick a failed script to run it again.
    #[test]
    fn a_finished_script_can_be_selected_again() {
        let mut script = item("Run Webroot Scan");
        script.status = ScriptStatus::Failed;

        script.select();
        assert!(script.is_selected(), "a failed script must be re-runnable");
        assert_eq!(script.status, ScriptStatus::Failed, "selecting is not a status change");

        script.deselect();
        assert!(!script.is_selected());

        script.status = ScriptStatus::Completed;
        script.toggle_selection();
        assert!(script.is_selected());
    }

    /// Stopping clears the running mark without touching what the tech ticked.
    #[test]
    fn stopping_keeps_the_selection() {
        let mut queue = ScriptQueue::new();
        let mut script = item("Run Webroot Scan");
        script.select();
        queue.add(script);

        queue.start();
        assert_eq!(queue.items()[0].script.status, ScriptStatus::Running);

        queue.stop();
        assert!(!queue.is_running());
        assert_eq!(queue.items()[0].script.status, ScriptStatus::Pending);
        assert!(
            queue.items()[0].script.is_selected(),
            "stopping must not untick the script"
        );
    }
}

#[cfg(test)]
mod log_cursor_tests {
    use super::*;
    use crate::scripts::ScriptCategory;

    fn entry(message: &str) -> ScriptLogEntry {
        ScriptLogEntry::info(ScriptCategory::Tuneup, "test", message)
    }

    #[test]
    fn the_log_stays_bounded() {
        let mut state = ScriptsState::new();
        for i in 0..(LOG_CAP * 3) {
            state.log(entry(&i.to_string()));
        }
        assert!(state.logs().len() <= LOG_CAP + LOG_TRIM_BATCH);
        let newest = (LOG_CAP * 3 - 1).to_string();
        assert_eq!(state.logs().last().map(|e| e.message.as_str()), Some(newest.as_str()));
    }

    /// A tracked run reads from its cursor; trimming must not shift it onto other entries.
    #[test]
    fn a_cursor_survives_trimming() {
        let mut state = ScriptsState::new();
        for i in 0..LOG_CAP {
            state.log(entry(&i.to_string()));
        }
        let cursor = state.log_cursor();
        state.log(entry("mine"));
        for i in 0..(LOG_TRIM_BATCH * 2) {
            state.log(entry(&format!("later {i}")));
        }
        let first = state.logs_since(cursor).first().map(|e| e.message.as_str());
        assert_eq!(first, Some("mine"));
    }

    #[test]
    fn clearing_keeps_cursors_valid() {
        let mut state = ScriptsState::new();
        state.log(entry("before"));
        let cursor = state.log_cursor();
        state.clear_logs();
        state.log(entry("after"));
        let since: Vec<&str> = state.logs_since(cursor).iter().map(|e| e.message.as_str()).collect();
        assert_eq!(since, ["after"]);
        assert!(state.logs_between(0, cursor).is_empty(), "dropped entries must not come back");
    }
}

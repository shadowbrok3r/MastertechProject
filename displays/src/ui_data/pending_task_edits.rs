//! Drives the staged-task-edit lifecycle once per frame: writes batches whose
//! idle window has elapsed, and keeps exactly one undo toast alive per task
//! with a pending batch.

use crate::app_state::SharedContext;
use crate::tabs::tasks::pending;
use crate::{PlatformSpawner, Spawner};
use database::schema::{LiveTaskPayload, RecordIdExt, TaskField};

impl SharedContext {
    pub fn tick_pending_task_edits(&mut self) {
        for edit in pending::take_due() {
            let fields: Vec<_> = edit.fields.iter().copied().collect();
            let task = edit.staged.clone();
            let name = task.task_name.clone();
            PlatformSpawner::spawn(async move {
                if let Err(e) = task.update_fields(&fields).await {
                    log::error!("staged task edits failed for {name}: {e:?}");
                    let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Error(
                        format!("Could not save changes to {name}: {e}"),
                    ));
                }
            });
        }

        // One toast per pending task, added on the first staged edit. The
        // renderer reads live state, so later edits need no new toast.
        for task_id in pending::pending_ids() {
            if self.undo_toasts_shown.contains(&task_id) {
                continue;
            }
            let name = self
                .task_index
                .get(&task_id)
                .map(|t| t.task_name.clone())
                .unwrap_or_else(|| task_id.clone());
            self.toasts.add(pending::undo_toast(&task_id, &name));
            self.undo_toasts_shown.insert(task_id);
        }

        self.undo_toasts_shown.retain(|id| pending::is_pending(id));
    }

    /// Clears every staged batch and its toast bookkeeping.
    pub fn reset_pending_task_edits(&mut self) {
        pending::clear();
        self.undo_toasts_shown.clear();
    }

    /// Drops the search and every trace of it.
    pub fn clear_task_search(&mut self) {
        self.search_results = None;
        self.task_search.reset();
    }
}

/// Stages one field edit and logs it, for call sites that hold a task but no
/// context handle.
pub fn stage_edit(live: &LiveTaskPayload, edited: &LiveTaskPayload, field: TaskField) {
    log::debug!("staging {} on {}", field.label(), live.id.key_string());
    pending::stage(live, edited, field);
}

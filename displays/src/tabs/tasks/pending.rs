//! Staged task edits: assignee / priority / due date / status / completion are
//! held for [`COMMIT_DELAY`] after the last change, then written as one UPDATE.
//! Cards render the staged values but stay in the column their pre-edit values
//! put them in, so retargeting a task does not move it out from under the
//! operator mid-edit.
//!
//! Global rather than a `SharedContext` field because the card controls live in
//! `Interaction`/`Displayable` impls on `LiveTaskPayload`, which have no handle
//! to the context — the same reason [`crate::get_database_users`] exists.

use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, ToastStyle};
use database::schema::{LiveTaskPayload, RecordIdExt, TaskField};
use eframe::egui::{
    Frame, Margin, ProgressBar, Response, RichText, Sense, Ui, Vec2,
};
use once_cell::sync::Lazy;
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use web_time::{Duration, Instant};

/// Idle time after the last staged edit before the batch is written.
pub const COMMIT_DELAY: Duration = Duration::from_secs(5);

/// How long a written batch keeps overlaying the card, waiting for the live
/// query to echo the new values back. Without it the card briefly snaps to its
/// old values between the write and the echo.
const ECHO_GRACE: Duration = Duration::from_secs(3);

/// One task's staged-but-unwritten edits.
#[derive(Clone, Debug)]
pub struct PendingEdit {
    /// Values before the first staged change. Drives column placement and undo.
    pub original: LiveTaskPayload,
    /// Values the operator has staged. Drives the card's controls.
    pub staged: LiveTaskPayload,
    pub fields: BTreeSet<TaskField>,
    last_touched: Instant,
    /// Set once the batch has been handed to the DB. The overlay lives on for
    /// [`ECHO_GRACE`], but undo is no longer offered.
    committed_at: Option<Instant>,
}

impl PendingEdit {
    /// Time left before this batch is written.
    pub fn remaining(&self) -> Duration {
        if self.committed_at.is_some() {
            return Duration::ZERO;
        }
        COMMIT_DELAY.saturating_sub(self.last_touched.elapsed())
    }

    /// 0.0 at the moment of the last edit, 1.0 once written.
    pub fn progress(&self) -> f32 {
        if self.committed_at.is_some() {
            return 1.0;
        }
        let elapsed = self.last_touched.elapsed().as_secs_f32();
        (elapsed / COMMIT_DELAY.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// True once the write has been issued; the overlay outlives it briefly.
    pub fn is_committed(&self) -> bool {
        self.committed_at.is_some()
    }

    /// Operator-facing list of what changed, e.g. "assignee, due date".
    pub fn summary(&self) -> String {
        let mut labels: Vec<&str> = self.fields.iter().map(TaskField::label).collect();
        labels.dedup();
        labels.join(", ")
    }
}

static PENDING: Lazy<Mutex<HashMap<String, PendingEdit>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Stages `field` from `edited`, seeding the undo baseline from `live` the
/// first time this task is touched. Resets the commit timer.
pub fn stage(live: &LiveTaskPayload, edited: &LiveTaskPayload, field: TaskField) {
    let Ok(mut map) = PENDING.lock() else {
        log::warn!("pending task edits: lock poisoned, staging skipped");
        return;
    };
    let key = live.id.key_string();
    let entry = map.entry(key).or_insert_with(|| PendingEdit {
        original: live.clone(),
        staged: live.clone(),
        fields: BTreeSet::new(),
        last_touched: Instant::now(),
        committed_at: None,
    });

    // Editing again during the echo grace restarts the batch from the values
    // just written, rather than reviving the pre-edit baseline.
    if entry.committed_at.is_some() {
        entry.original = entry.staged.clone();
        entry.fields.clear();
        entry.committed_at = None;
    }

    // Copy only the edited field so a stale clone can't revert a sibling edit.
    match field {
        TaskField::Assignee => entry.staged.assignee = edited.assignee.clone(),
        TaskField::Priority => entry.staged.priority = edited.priority.clone(),
        TaskField::DueDate => entry.staged.due_date = edited.due_date.clone(),
        TaskField::Status => entry.staged.status = edited.status.clone(),
        TaskField::Completed => {
            entry.staged.completed = edited.completed;
            entry.staged.status = edited.status.clone();
        }
    }
    entry.fields.insert(field);
    entry.last_touched = Instant::now();
}

/// The staged edit for `task_id`, if one is pending.
pub fn get(task_id: &str) -> Option<PendingEdit> {
    PENDING.lock().ok()?.get(task_id).cloned()
}

/// True when an edit is staged but not yet written for `task_id`. Batches
/// inside the echo grace read as false: they are already committed.
pub fn is_pending(task_id: &str) -> bool {
    PENDING
        .lock()
        .map(|m| m.get(task_id).is_some_and(|e| !e.is_committed()))
        .unwrap_or(false)
}

/// Overlays staged values onto `task` so a card renders what the operator
/// picked while the write is still held.
pub fn apply_staged(task: &mut LiveTaskPayload) {
    let Some(edit) = get(&task.id.key_string()) else {
        return;
    };
    for field in &edit.fields {
        match field {
            TaskField::Assignee => task.assignee = edit.staged.assignee.clone(),
            TaskField::Priority => task.priority = edit.staged.priority.clone(),
            TaskField::DueDate => task.due_date = edit.staged.due_date.clone(),
            TaskField::Status => task.status = edit.staged.status.clone(),
            TaskField::Completed => {
                task.completed = edit.staged.completed;
                task.status = edit.staged.status.clone();
            }
        }
    }
}

/// Drops the staged batch without writing anything. Returns what was
/// discarded, or `None` if the write has already gone out.
pub fn cancel(task_id: &str) -> Option<PendingEdit> {
    let mut map = PENDING.lock().ok()?;
    if map.get(task_id)?.is_committed() {
        return None;
    }
    map.remove(task_id)
}

/// Marks every batch whose idle window has elapsed as committed and returns it
/// for writing. The entry survives [`ECHO_GRACE`] so the card keeps showing the
/// written values until the live query catches up.
pub fn take_due() -> Vec<PendingEdit> {
    let Ok(mut map) = PENDING.lock() else {
        return Vec::new();
    };

    map.retain(|_, e| {
        e.committed_at
            .is_none_or(|at| at.elapsed() < ECHO_GRACE)
    });

    let now = Instant::now();
    let mut due = Vec::new();
    for edit in map.values_mut() {
        if edit.committed_at.is_none() && edit.remaining().is_zero() {
            edit.committed_at = Some(now);
            due.push(edit.clone());
        }
    }
    due
}

/// Task ids with a batch staged but not yet written.
pub fn pending_ids() -> Vec<String> {
    PENDING
        .lock()
        .map(|m| {
            m.iter()
                .filter(|(_, e)| !e.is_committed())
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Clears everything, e.g. on store switch or sign-out, so staged edits never
/// land against a board the operator has left.
pub fn clear() {
    if let Ok(mut map) = PENDING.lock() {
        map.clear();
    }
}

/// Ages every staged batch past its commit delay so a test can drive the
/// commit without sleeping.
#[cfg(test)]
fn force_due() {
    if let Ok(mut map) = PENDING.lock() {
        for edit in map.values_mut() {
            edit.last_touched = Instant::now() - COMMIT_DELAY - Duration::from_millis(1);
        }
    }
}

/// `ToastKind::Custom` discriminant for the undo toast.
pub const UNDO_TOAST_KIND: u32 = 0x7A5C_0001;

/// The toast offering to undo the batch staged on `task_id`. Lives until the
/// batch commits or is undone, so the operator can keep editing without the
/// notification expiring underneath them.
pub fn undo_toast(task_id: &str, task_name: &str) -> Toast {
    Toast {
        kind: ToastKind::Custom(UNDO_TOAST_KIND),
        text: task_name.to_string().into(),
        options: ToastOptions::default().show_icon(false).show_progress(false),
        style: ToastStyle::default(),
        user_dismissed: false,
        payload: Some(task_id.to_string()),
    }
}

/// Renders the undo toast. Reads live pending state each frame so the field
/// list and countdown track further edits, and closes itself once the batch is
/// no longer pending.
pub fn undo_toast_contents(ui: &mut Ui, toast: &mut Toast) -> Response {
    let task_id = toast.payload.clone().unwrap_or_default();
    let edit = get(&task_id).filter(|e| !e.is_committed());
    let Some(edit) = edit else {
        // Written or already undone — nothing left to offer.
        toast.close();
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    };

    let task_name = toast.text.text().to_string();
    let secs = edit.remaining().as_secs_f32().ceil() as u32;

    // The toast has an infinite TTL, so nothing else drives the countdown.
    ui.ctx().request_repaint();

    Frame::window(ui.style())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("Changed {} · {task_name}", edit.summary()))
                        .strong(),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("applying in {secs}s"))
                            .small()
                            .weak(),
                    );
                    ui.add_space(6.0);
                    if ui
                        .button(format!("{} Undo", crate::ui_tools::icons::UNDO))
                        .clicked()
                    {
                        cancel(&task_id);
                        toast.close();
                    }
                });
                // The countdown has to keep ticking on screen.
                ui.add(ProgressBar::new(1.0 - edit.progress()).desired_height(2.0));
            });
        })
        .response
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::{Priority, Status};

    /// PENDING is process-global, so the tests that clear it must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Holds the serial lock and leaves PENDING empty on the way in and out.
    struct Isolated(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Isolated {
        fn new() -> Self {
            let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            clear();
            Self(guard)
        }
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            clear();
        }
    }

    fn task(name: &str) -> LiveTaskPayload {
        let mut t = LiveTaskPayload::default();
        t.task_name = name.to_string();
        t
    }

    #[test]
    fn staging_records_only_the_edited_field() {
        let _guard = Isolated::new();
        let live = task("a");
        let mut edited = live.clone();
        edited.priority = Priority::Fire;
        edited.status = Status::Complete; // untouched field must not carry over
        stage(&live, &edited, TaskField::Priority);

        let edit = get(&live.id.key_string()).expect("staged");
        assert_eq!(edit.fields.len(), 1);
        assert_eq!(edit.staged.priority, Priority::Fire);
        assert_eq!(edit.staged.status, live.status);
        assert_eq!(edit.original.priority, live.priority);
    }

    #[test]
    fn multiple_fields_coalesce_into_one_batch() {
        let _guard = Isolated::new();
        let live = task("b");
        let mut edited = live.clone();
        edited.priority = Priority::Rfs;
        stage(&live, &edited, TaskField::Priority);
        edited.completed = true;
        edited.status = Status::Complete;
        stage(&live, &edited, TaskField::Completed);

        let edit = get(&live.id.key_string()).expect("staged");
        assert_eq!(edit.fields.len(), 2);
        assert_eq!(edit.summary(), "priority, completion");
        assert_eq!(pending_ids().len(), 1);
    }

    #[test]
    fn apply_staged_overlays_only_staged_fields() {
        let _guard = Isolated::new();
        let live = task("c");
        let mut edited = live.clone();
        edited.completed = true;
        edited.status = Status::Complete;
        stage(&live, &edited, TaskField::Completed);

        let mut card = live.clone();
        card.priority = Priority::Qc; // not staged, must survive the overlay
        apply_staged(&mut card);
        assert!(card.completed);
        assert_eq!(card.status, Status::Complete);
        assert_eq!(card.priority, Priority::Qc);
    }

    #[test]
    fn cancel_discards_without_committing() {
        let _guard = Isolated::new();
        let live = task("d");
        let mut edited = live.clone();
        edited.completed = true;
        stage(&live, &edited, TaskField::Completed);

        let undone = cancel(&live.id.key_string()).expect("cancelled");
        assert!(undone.staged.completed);
        assert!(!undone.original.completed);
        assert!(!is_pending(&live.id.key_string()));
        assert!(take_due().is_empty());
    }

    #[test]
    fn a_due_batch_commits_once_and_stops_offering_undo() {
        let _guard = Isolated::new();
        let live = task("f");
        let mut edited = live.clone();
        edited.completed = true;
        edited.status = Status::Complete;
        stage(&live, &edited, TaskField::Completed);

        force_due();
        let due = take_due();
        assert_eq!(due.len(), 1, "the batch should be handed over exactly once");
        assert!(due[0].staged.completed);

        let key = live.id.key_string();
        // Committed: undo is gone and no toast should be offered...
        assert!(cancel(&key).is_none());
        assert!(!is_pending(&key));
        assert!(pending_ids().is_empty());
        // ...but the overlay lives on so the card does not snap back.
        let mut card = live.clone();
        apply_staged(&mut card);
        assert!(card.completed, "overlay must survive the echo grace");
        // And it is never written twice.
        assert!(take_due().is_empty());
    }

    #[test]
    fn a_committed_batch_reads_as_finished() {
        let _guard = Isolated::new();
        let live = task("g");
        let mut edited = live.clone();
        edited.priority = Priority::Fire;
        stage(&live, &edited, TaskField::Priority);
        force_due();
        take_due();

        let edit = get(&live.id.key_string()).expect("still overlaying");
        assert!(edit.is_committed());
        assert_eq!(edit.progress(), 1.0, "a written ring reads as full");
        assert!(edit.remaining().is_zero());
    }

    #[test]
    fn editing_during_the_echo_grace_starts_a_fresh_batch() {
        let _guard = Isolated::new();
        let live = task("h");
        let mut edited = live.clone();
        edited.priority = Priority::Fire;
        stage(&live, &edited, TaskField::Priority);
        force_due();
        take_due();

        // A second edit must not resurrect the pre-edit baseline.
        let mut again = edited.clone();
        again.completed = true;
        again.status = Status::Complete;
        stage(&live, &again, TaskField::Completed);

        let key = live.id.key_string();
        let edit = get(&key).expect("restaged");
        assert!(!edit.is_committed());
        assert_eq!(edit.fields.len(), 1, "only the new field is pending");
        assert_eq!(
            edit.original.priority,
            Priority::Fire,
            "baseline is what was written, not the original priority"
        );
        assert!(is_pending(&key));
    }

    #[test]
    fn fresh_batch_is_not_yet_due() {
        let _guard = Isolated::new();
        let live = task("e");
        let mut edited = live.clone();
        edited.priority = Priority::Express;
        stage(&live, &edited, TaskField::Priority);
        assert!(take_due().is_empty());
        assert!(is_pending(&live.id.key_string()));
    }
}

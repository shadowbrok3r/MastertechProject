//! Per-frame drain for the AI-task snapshot + live streams.
//!
//! Feeds `SharedContext.ai_tasks` / `ai_task_items` (the single source of
//! truth for card, column, and diagnostics-tab checklists) and queues the
//! blocking attention/review popups on state transitions.

use crate::app_state::SharedContext;
use crate::modals::ai_attention_modal::{AiPopup, AiPopupKind};
use database::live_data::Action;
use database::schema::{AiTask, AiTaskStatus, RecordIdExt};

impl SharedContext {
    /// Queue a popup unless one for the same ai_task + kind is already
    /// queued or currently showing.
    fn queue_ai_popup(&mut self, kind: AiPopupKind, task: &AiTask) {
        let key = task.id.key_string();
        let already_queued = self
            .ai_popup_queue
            .iter()
            .any(|p| p.kind == kind && p.ai_task.id.key_string() == key);
        let currently_shown = self
            .ai_popup_modal
            .as_ref()
            .map(|m| m.popup.kind == kind && m.popup.ai_task.id.key_string() == key)
            .unwrap_or(false);
        if already_queued || currently_shown {
            return;
        }
        let item_count = self
            .ai_task_items
            .values()
            .filter(|i| i.ai_task_ref.key_string() == key)
            .count();
        self.ai_popup_queue.push_back(AiPopup {
            kind,
            ai_task: task.clone(),
            item_count,
        });
    }

    /// Popup rules for an incoming ai_task row against the prior local state.
    fn detect_ai_popups(&mut self, task: &AiTask, previous: Option<&AiTask>) {
        let Some(me) = self.current_user.as_ref().map(|u| u.get_id()) else {
            return;
        };

        // Tech attention: open, unacknowledged, mine — fresh row or a
        // reopen/reassign (previous state acknowledged or not open).
        let needs_attention = task.assignee == me
            && task.status == AiTaskStatus::Open
            && task.acknowledged_at.is_none()
            && previous
                .map(|old| old.acknowledged_at.is_some() || old.status != AiTaskStatus::Open || old.assignee != me)
                .unwrap_or(true);
        if needs_attention {
            self.queue_ai_popup(AiPopupKind::TechAttention, task);
        }

        // Operator review: newly awaiting follow-up, unacknowledged, requested by me.
        let needs_review = task.requested_by == me
            && task.status == AiTaskStatus::AwaitingFollowup
            && task.review_acknowledged_at.is_none()
            && previous
                .map(|old| old.status != AiTaskStatus::AwaitingFollowup)
                .unwrap_or(true);
        if needs_review {
            self.queue_ai_popup(AiPopupKind::OperatorReview, task);
        }

        // Completion grace: the card lingers briefly on the tech board.
        if task.status == AiTaskStatus::AwaitingFollowup
            && previous.map(|old| old.status == AiTaskStatus::Open).unwrap_or(false)
        {
            self.ai_task_done_grace
                .insert(task.id.key_string(), web_time::Instant::now());
        }
    }

    pub fn receive_ai_task(&mut self) {
        // Snapshot replaces local state wholesale (login / reconnect refetch).
        if let Ok((tasks, items)) = self.initial_ai_tasks_rx.try_recv() {
            self.ai_tasks.clear();
            self.ai_task_items.clear();
            for item in items {
                self.ai_task_items.insert(item.id.key_string(), item);
            }
            for task in tasks {
                self.detect_ai_popups(&task, None);
                self.ai_tasks.insert(task.id.key_string(), task);
            }
        }

        while let Ok((action, task)) = self.live_ai_tasks_rx.try_recv() {
            let key = task.id.key_string();
            match action {
                Action::Create | Action::Update => {
                    let previous = self.ai_tasks.get(&key).cloned();
                    self.detect_ai_popups(&task, previous.as_ref());
                    self.ai_tasks.insert(key, task);
                }
                Action::Delete => {
                    self.ai_tasks.remove(&key);
                    self.ai_task_items
                        .retain(|_, i| i.ai_task_ref.key_string() != key);
                    self.ai_task_done_grace.remove(&key);
                }
            }
        }

        while let Ok((action, item)) = self.live_ai_task_items_rx.try_recv() {
            let key = item.id.key_string();
            match action {
                Action::Create | Action::Update => {
                    self.ai_task_items.insert(key, item);
                }
                Action::Delete => {
                    self.ai_task_items.remove(&key);
                }
            }
        }
    }
}

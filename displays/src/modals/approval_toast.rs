//! Top-right toasts that tell an active Root about approvals owned by someone else, with Review and Dismiss.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use database::schema::{RecordId, RecordIdExt};
use eframe::egui::{Align, Frame, Layout, Margin, Response, RichText, Sense, Ui, Vec2};

use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts};
use crate::ui_tools::{do_not_disturb, icons, theme};

/// `ToastKind::Custom` discriminant for approval toasts.
pub const APPROVAL_TOAST_KIND: u32 = 0x7A5C_0003;

const TOAST_WIDTH: f32 = 340.0;

/// What the approval asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Tool,
    Question,
    Sql,
}

impl NoticeKind {
    fn heading(self) -> (&'static str, &'static str) {
        match self {
            Self::Tool => (icons::LOCK, "Approval waiting"),
            Self::Question => (icons::CHAT, "Agent question"),
            Self::Sql => (icons::LOCK, "SurrealQL approval waiting"),
        }
    }
}

/// One toast's content.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalNotice {
    pub id: RecordId,
    pub kind: NoticeKind,
    pub summary: String,
    /// Machine or origin host shown under the summary.
    pub machine: Option<String>,
    /// No technician owns the request.
    pub unowned: bool,
}

/// Notices on screen and the clicks their toasts recorded.
#[derive(Default)]
struct Board {
    notices: HashMap<String, ApprovalNotice>,
    review: Vec<RecordId>,
    dismissed: Vec<RecordId>,
}

static BOARD: LazyLock<Mutex<Board>> = LazyLock::new(Mutex::default);

fn with_board<R>(f: impl FnOnce(&mut Board) -> R) -> Option<R> {
    BOARD.lock().ok().map(|mut board| f(&mut board))
}

/// `table:key` for a record id.
fn key_of(id: &RecordId) -> String {
    format!("{}:{}", id.table.as_str(), id.key_string())
}

/// Whether a toast may be posted while Do Not Disturb is `dnd`.
pub fn should_post(dnd: bool) -> bool {
    !dnd
}

/// Queues a toast for `notice`; false, with nothing posted, while Do Not Disturb is on.
pub fn post(toasts: &mut Toasts, notice: ApprovalNotice) -> bool {
    if !should_post(do_not_disturb::is_enabled()) {
        return false;
    }
    let key = key_of(&notice.id);
    let text = notice.summary.clone();
    with_board(|b| b.notices.insert(key.clone(), notice));
    toasts.add(Toast {
        kind: ToastKind::Custom(APPROVAL_TOAST_KIND),
        text: text.into(),
        options: ToastOptions::default().show_icon(false).show_progress(false),
        payload: Some(key),
        ..Default::default()
    });
    true
}

/// Closes the toast for `id` on its next draw.
pub fn retire(id: &RecordId) {
    let key = key_of(id);
    with_board(|b| b.notices.remove(&key));
}

/// A Review click on a `table` toast, handed out once.
pub fn take_review(table: &str) -> Option<RecordId> {
    with_board(|b| {
        let at = b.review.iter().position(|id| id.table.as_str() == table)?;
        Some(b.review.remove(at))
    })
    .flatten()
}

/// Dismiss clicks on `table` toasts since the last call.
pub fn take_dismissed(table: &str) -> Vec<RecordId> {
    with_board(|b| {
        let (taken, kept) = std::mem::take(&mut b.dismissed).into_iter().partition(|id| id.table.as_str() == table);
        b.dismissed = kept;
        taken
    })
    .unwrap_or_default()
}

/// Retires every `table` toast and drops its pending clicks.
pub fn reset(table: &str) {
    with_board(|b| {
        b.notices.retain(|_, n| n.id.table.as_str() != table);
        b.review.retain(|id| id.table.as_str() != table);
        b.dismissed.retain(|id| id.table.as_str() != table);
    });
}

/// Draws an approval toast; registered on the shared [`Toasts`] under [`APPROVAL_TOAST_KIND`].
pub fn toast_contents(ui: &mut Ui, toast: &mut Toast) -> Response {
    let key = toast.payload.clone().unwrap_or_default();
    let Some(notice) = with_board(|b| b.notices.get(&key).cloned()).flatten() else {
        toast.close();
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    };

    let (glyph, title) = notice.kind.heading();
    let mut review = false;
    let mut dismiss = false;
    let response = Frame::window(ui.style())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(TOAST_WIDTH);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{glyph} {title}")).strong().color(theme::warn(ui)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    dismiss = ui.small_button(icons::CLOSE).on_hover_text("Dismiss").clicked();
                });
            });
            ui.label(&notice.summary);
            if let Some(machine) = &notice.machine {
                ui.label(RichText::new(machine).small().color(theme::weak_text(ui)));
            }
            if notice.unowned {
                ui.label(RichText::new(format!("{} No technician on this session", icons::USER)).small());
            }
            ui.add_space(4.0);
            review = ui.button(format!("{} Review", icons::EYE)).clicked();
        })
        .response;

    if review || dismiss {
        with_board(|b| {
            b.notices.remove(&key);
            if review {
                b.review.push(notice.id.clone());
            } else {
                b.dismissed.push(notice.id.clone());
            }
        });
        toast.close();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(table: &str, key: &str) -> ApprovalNotice {
        ApprovalNotice {
            id: RecordId::new(table, key),
            kind: NoticeKind::Tool,
            summary: "run desktop_click on PC-1".into(),
            machine: Some("PC-1:abc".into()),
            unowned: false,
        }
    }

    #[test]
    fn nothing_is_posted_under_do_not_disturb() {
        assert!(should_post(false));
        assert!(!should_post(true));
    }

    #[test]
    fn a_review_click_is_handed_out_once_and_only_for_its_table() {
        let id = RecordId::new("toast_review_a", "review-once");
        with_board(|b| b.review.push(id.clone()));
        assert_eq!(take_review("toast_review_b"), None);
        assert_eq!(take_review("toast_review_a"), Some(id));
        assert_eq!(take_review("toast_review_a"), None);
    }

    #[test]
    fn dismissals_are_drained_per_table() {
        let first = RecordId::new("toast_dismiss_a", "dismiss-a");
        let second = RecordId::new("toast_dismiss_b", "dismiss-b");
        with_board(|b| b.dismissed.extend([first.clone(), second.clone()]));
        assert!(take_dismissed("toast_dismiss_a").contains(&first));
        assert!(!take_dismissed("toast_dismiss_a").contains(&first));
        assert!(take_dismissed("toast_dismiss_b").contains(&second));
    }

    fn draw(toast: &mut Toast) {
        let ctx = eframe::egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            toast_contents(ui, toast);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn a_retired_notice_closes_its_toast() {
        let n = notice("toast_retired", "retired");
        let key = key_of(&n.id);
        with_board(|b| b.notices.insert(key.clone(), n.clone()));
        let mut toast = Toast { payload: Some(key), ..Default::default() };
        draw(&mut toast);
        assert!(toast.options.ttl_sec > 0.0, "a live notice keeps its toast");
        retire(&n.id);
        draw(&mut toast);
        assert!(toast.options.ttl_sec <= 0.0, "a retired notice closes its toast");
    }

    #[test]
    fn keys_carry_the_table() {
        assert_eq!(key_of(&RecordId::new("sql_approval", "abc")), "sql_approval:abc");
    }
}

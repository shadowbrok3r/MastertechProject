//! The toast that offers a downloaded MasterTech update, with Update now and Later.

use std::sync::{LazyLock, Mutex};

use eframe::egui::{Frame, Margin, Response, RichText, Sense, Ui, Vec2};

use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts};
use crate::ui_tools::{do_not_disturb, icons, theme};

/// `ToastKind::Custom` discriminant for the update toast.
pub const UPDATE_TOAST_KIND: u32 = 0x7A5C_0005;

const TOAST_WIDTH: f32 = 320.0;

/// The technician's answer to the update toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateChoice {
    Now,
    Later,
}

/// The version on offer and the click its toast recorded.
#[derive(Default)]
struct Board {
    offered: Option<String>,
    choice: Option<UpdateChoice>,
}

static BOARD: LazyLock<Mutex<Board>> = LazyLock::new(Mutex::default);

fn with_board<R>(f: impl FnOnce(&mut Board) -> R) -> Option<R> {
    BOARD.lock().ok().map(|mut board| f(&mut board))
}

/// Whether a toast may be posted while Do Not Disturb is `dnd` and one is already `shown`.
pub fn should_post(dnd: bool, shown: bool) -> bool {
    !dnd && !shown
}

/// Offers `version`; false, with nothing posted, while Do Not Disturb is on or the toast is already up.
pub fn post(toasts: &mut Toasts, version: &str) -> bool {
    if !should_post(do_not_disturb::is_enabled(), is_shown()) {
        return false;
    }
    with_board(|b| {
        b.offered = Some(version.to_string());
        b.choice = None;
    });
    toasts.add(Toast {
        kind: ToastKind::Custom(UPDATE_TOAST_KIND),
        text: format!("MasterTech {version} is ready").into(),
        options: ToastOptions::default()
            .show_icon(false)
            .show_progress(false),
        ..Default::default()
    });
    true
}

/// Whether the update toast is on screen.
pub fn is_shown() -> bool {
    with_board(|b| b.offered.is_some()).unwrap_or(false)
}

/// The technician's click, handed out once.
pub fn take_choice() -> Option<UpdateChoice> {
    with_board(|b| b.choice.take()).flatten()
}

/// Closes the toast on its next draw.
pub fn retire() {
    with_board(|b| b.offered = None);
}

/// Draws the update toast; registered on the shared [`Toasts`] under [`UPDATE_TOAST_KIND`].
pub fn toast_contents(ui: &mut Ui, toast: &mut Toast) -> Response {
    let Some(version) = with_board(|b| b.offered.clone()).flatten() else {
        toast.close();
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    };

    let mut choice = None;
    let response = Frame::window(ui.style())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(TOAST_WIDTH);
            ui.label(
                RichText::new(format!("{} MasterTech {version} is ready", icons::DOWNLOAD))
                    .strong()
                    .color(theme::accent(ui)),
            );
            ui.label("Installing it restarts MasterTech.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Update now", icons::REFRESH))
                    .clicked()
                {
                    choice = Some(UpdateChoice::Now);
                }
                if ui.button("Later").clicked() {
                    choice = Some(UpdateChoice::Later);
                }
            });
        })
        .response;

    if let Some(choice) = choice {
        with_board(|b| {
            b.offered = None;
            b.choice = Some(choice);
        });
        toast.close();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    static BOARD_LOCK: Mutex<()> = Mutex::new(());

    fn draw(toast: &mut Toast) {
        let ctx = eframe::egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            toast_contents(ui, toast);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn nothing_is_posted_under_do_not_disturb_or_twice() {
        assert!(should_post(false, false));
        assert!(!should_post(true, false));
        assert!(!should_post(false, true));
    }

    #[test]
    fn a_choice_is_handed_out_once() {
        let _lock = BOARD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        with_board(|b| b.choice = Some(UpdateChoice::Later));
        assert_eq!(take_choice(), Some(UpdateChoice::Later));
        assert_eq!(take_choice(), None);
    }

    #[test]
    fn the_toast_stays_while_offered_and_closes_once_retired() {
        let _lock = BOARD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        with_board(|b| b.offered = Some("v4.8.12".into()));
        let mut toast = Toast::default();
        draw(&mut toast);
        assert!(
            toast.options.ttl_sec > 0.0,
            "an offered update keeps its toast"
        );
        assert!(is_shown());
        retire();
        draw(&mut toast);
        assert!(
            toast.options.ttl_sec <= 0.0,
            "a retired offer closes its toast"
        );
        assert!(!is_shown());
    }
}

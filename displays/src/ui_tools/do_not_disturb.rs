//! Process-global Do Not Disturb switch for notification surfaces.
//!
//! Gates toasts, the admin notification modal, and the AI attention popup
//! pump. Approval and confirmation dialogs are never gated — an operator must
//! still be able to action a pending mutation. State lives in a process
//! static, so it is never persisted and clears on restart.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use eframe::egui::{vec2, Button, Ui};

use crate::ui_tools::{icons, theme};

static ENABLED: AtomicBool = AtomicBool::new(false);
static SUPPRESSED: AtomicUsize = AtomicUsize::new(0);

/// True while notification surfaces are silenced.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Sets the switch, clearing the silenced tally on a state change.
pub fn set_enabled(enabled: bool) {
    if ENABLED.swap(enabled, Ordering::Relaxed) != enabled {
        SUPPRESSED.store(0, Ordering::Relaxed);
    }
}

/// Notifications dropped since the switch was last toggled.
pub fn suppressed_count() -> usize {
    SUPPRESSED.load(Ordering::Relaxed)
}

/// Records one dropped notification.
pub fn note_suppressed() {
    SUPPRESSED.fetch_add(1, Ordering::Relaxed);
}

/// True when the caller's notification is silenced, counting the drop.
pub fn silenced() -> bool {
    let on = is_enabled();
    if on {
        note_suppressed();
    }
    on
}

/// Menu-bar bell toggling the switch; carries the silenced tally while on.
pub fn toggle_button(ui: &mut Ui) {
    let on = is_enabled();
    let count = suppressed_count();

    let glyph = if on { icons::BELL_SLASH } else { icons::BELL };
    let label = if on && count > 0 {
        format!("{glyph} {count}")
    } else {
        glyph.to_string()
    };
    let (tint, fill) = if on {
        (theme::error(ui), theme::error(ui).gamma_multiply(0.25))
    } else {
        (
            ui.style().visuals.text_color(),
            ui.style().visuals.widgets.inactive.weak_bg_fill,
        )
    };

    let hover = if on {
        format!(
            "Do Not Disturb is ON — {count} notification(s) silenced.\nClick to resume notifications."
        )
    } else {
        "Silence all notifications until the app restarts.\nApproval and confirmation dialogs still appear.".to_string()
    };

    let button = Button::new(icons::icon(&label).color(tint).small().strong())
        .fill(fill)
        .corner_radius(10.0)
        .min_size(vec2(28.0, 18.0));
    if ui.add(button).on_hover_text(hover).clicked() {
        set_enabled(!on);
    }
}

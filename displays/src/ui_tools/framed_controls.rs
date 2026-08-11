//! Selectable controls that keep a frame when they are not selected.
//!
//! A tab strip built from `Ui::selectable_label` reads as plain text until the operator happens to
//! hover it. That is not the theme: `selectable_label` forwards to `Button::selectable`, which sets
//! `frame_when_inactive(selected)`, and `Button` skips the frame entirely when it is both inactive
//! and unselected. Only the padding is left, so `widgets.inactive.bg_stroke` never paints.
//!
//! `MenuBar` loses the same frame from the other direction. Its default style modifier,
//! [`eframe::egui::containers::menu::menu_style`], clears `widgets.inactive.weak_bg_fill` and every
//! `bg_stroke` in the bar, so menu buttons and comboboxes inside it are painted frameless no matter
//! what the theme asked for.
//!
//! Neither is reachable from a saved theme — `frame_when_inactive` is a per-widget flag, and the
//! menu style overwrites the theme after it is applied. [`FramedSelectable`] and
//! [`framed_menu_style`] hand both frames back to the theme.

use eframe::egui::{Button, IntoAtoms, Response, Style, Ui, Vec2, Widget};

/// Button padding inside a menu bar; egui's `menu_style` uses a tighter 2×0.
const MENU_BAR_BUTTON_PADDING: Vec2 = Vec2::new(5.0, 1.0);

/// `MenuBar::style` replacement that leaves every widget visual to the theme.
pub fn framed_menu_style(style: &mut Style) {
    style.spacing.button_padding = MENU_BAR_BUTTON_PADDING;
}

/// Selectable toggles that paint the theme's `widgets.inactive` frame while unselected.
pub trait FramedSelectable {
    /// [`Ui::selectable_label`] that keeps its frame when unselected.
    fn framed_selectable_label<'a>(&mut self, selected: bool, text: impl IntoAtoms<'a>) -> Response;

    /// [`Ui::selectable_value`] that keeps its frame when unselected.
    fn framed_selectable_value<'a, V: PartialEq>(
        &mut self,
        current_value: &mut V,
        selected_value: V,
        text: impl IntoAtoms<'a>,
    ) -> Response;
}

impl FramedSelectable for Ui {
    fn framed_selectable_label<'a>(&mut self, selected: bool, text: impl IntoAtoms<'a>) -> Response {
        Button::new(text)
            .selected(selected)
            .frame(true)
            .frame_when_inactive(true)
            .ui(self)
    }

    fn framed_selectable_value<'a, V: PartialEq>(
        &mut self,
        current_value: &mut V,
        selected_value: V,
        text: impl IntoAtoms<'a>,
    ) -> Response {
        let mut response = self.framed_selectable_label(*current_value == selected_value, text);
        if response.clicked() && *current_value != selected_value {
            *current_value = selected_value;
            response.mark_changed();
        }
        response
    }
}

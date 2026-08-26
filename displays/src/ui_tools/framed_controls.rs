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

use eframe::egui::{
    AsIdSalt, Button, Frame, InnerResponse, IntoAtoms, Margin, Response, Sense, Style, Ui,
    UiBuilder, Vec2, Widget,
};

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

/// A full-width, framed, multi-line row that responds to a click anywhere inside it.
///
/// [`FramedSelectable::framed_selectable_label`] is a `Button`, so its content is
/// one run of atoms; a list row that carries a heading, an id and a couple of
/// metric lines needs a container. The scope senses the click itself, which is
/// why every widget drawn inside must be non-interactive — a nested button would
/// swallow the row's click.
pub fn selectable_card<R>(
    ui: &mut Ui,
    id_salt: impl AsIdSalt,
    selected: bool,
    contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let InnerResponse { inner, response } = ui.scope_builder(
        UiBuilder::new().id_salt(id_salt).sense(Sense::click()),
        |ui| {
            let visuals = ui.style().interact_selectable(&ui.response(), selected);
            Frame::new()
                .fill(visuals.weak_bg_fill)
                .stroke(visuals.bg_stroke)
                .corner_radius(visuals.corner_radius)
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let style = ui.style_mut();
                    // `selectable_labels` gives every contained label a click
                    // sense for text selection, which would consume the row's
                    // own click before the scope sees it.
                    style.interaction.selectable_labels = false;
                    style.visuals.override_text_color = Some(visuals.text_color());
                    contents(ui)
                })
                .inner
        },
    );
    InnerResponse::new(inner, response)
}

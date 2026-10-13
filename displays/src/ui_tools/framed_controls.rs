//! Framed selectable rows, and a menu-bar style that keeps the theme's widget frames.

use eframe::egui::{AsIdSalt, Frame, InnerResponse, Margin, Sense, Style, Ui, UiBuilder, Vec2};

/// Button padding inside a menu bar; egui's `menu_style` uses a tighter 2×0.
const MENU_BAR_BUTTON_PADDING: Vec2 = Vec2::new(5.0, 1.0);

/// `MenuBar::style` replacement that leaves every widget visual to the theme.
pub fn framed_menu_style(style: &mut Style) {
    style.spacing.button_padding = MENU_BAR_BUTTON_PADDING;
}

/// A framed full-width row clickable anywhere; its contents must be non-interactive.
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

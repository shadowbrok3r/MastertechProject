//! Owned control kit: fixed-height, self-painted controls that read [`Tokens`] and [`Metrics`].

mod buttons;
mod fields;
mod gallery;
mod labels;
mod layout;
mod list;
mod meter;
mod toggle;

use std::sync::Arc;

use eframe::egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Galley, Id, Rect, Response, Stroke,
    StrokeKind, TextStyle, Ui, text::LayoutJob,
};

pub use crate::icon_pack::Icon;
pub use crate::metrics::Metrics;
pub use crate::tokens::Tokens;
pub use buttons::{
    ButtonKind, IconBox, IconButton, TextButton, button, danger_button, ghost_button, icon_button,
    icon_text_button, primary_button, secondary_button,
};
pub use fields::{
    Dropdown, TextArea, TextField, ValueField, dropdown, search_field, text_area, text_field,
    value_field,
};
pub use gallery::gallery;
pub use labels::{
    IconLabel, IconLink, Pill, Section, SpinnerSize, badge, count_badge, elided_label, fold_header,
    hairline, heading, icon_hyperlink, icon_label, icon_link, loading_row, note, pill,
    section_header, section_label, section_label_icon, spinner, status_label, subheading,
    tinted_spinner, vline,
};
pub use layout::{
    BarKind, MenuButton, MenuItem, ModalOutcome, ModalWidth, Role, bar, callout, card, card_frame,
    dialog_buttons, dialog_buttons_enabled, dialog_contents, empty_state, icon_window,
    icon_window_tone, loading_state, menu_body, menu_check_item, menu_danger_item, menu_inset_row,
    menu_item, menu_picker, menu_separator, modal, panel_header, popup_menu, submenu, tab_title,
    window_id,
};
pub use list::ListItem;
pub use meter::{ProgressBar, Segment, Slider, meter, progress_bar, segmented, slider};
pub use toggle::{checkbox, toggle};

pub(crate) use layout::menu_rows;

/// Interaction state a control paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum State {
    Rest,
    Hovered,
    Pressed,
}

/// A state the gallery forces on the controls it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Preview {
    Hovered,
    Pressed,
    Focused,
}

fn preview_id() -> Id {
    Id::unique("mtech.controls.preview")
}

/// Runs `add` with every kit control painting the `preview` state.
pub(crate) fn with_preview<R>(ui: &mut Ui, preview: Preview, add: impl FnOnce(&mut Ui) -> R) -> R {
    let before = ui.data(|d| d.get_temp::<Preview>(preview_id()));
    ui.data_mut(|d| d.insert_temp(preview_id(), preview));
    let out = add(ui);
    ui.data_mut(|d| {
        if let Some(p) = before {
            d.insert_temp(preview_id(), p);
        } else {
            d.remove::<Preview>(preview_id());
        }
    });
    out
}

/// The painted state of `response` and whether it shows a keyboard-focus ring.
pub(crate) fn state_of(ui: &Ui, response: &Response) -> (State, bool) {
    if !ui.is_enabled() {
        return (State::Rest, false);
    }
    match ui.data(|d| d.get_temp::<Preview>(preview_id())) {
        Some(Preview::Hovered) => (State::Hovered, false),
        Some(Preview::Pressed) => (State::Pressed, false),
        Some(Preview::Focused) => (State::Rest, true),
        None => {
            let state = if response.is_pointer_button_down_on() {
                State::Pressed
            } else if response.hovered() {
                State::Hovered
            } else {
                State::Rest
            };
            (state, response.has_focus())
        }
    }
}

/// Whether the gallery forces a focused look on the controls it draws.
pub(crate) fn preview_focused(ui: &Ui) -> bool {
    ui.data(|d| d.get_temp::<Preview>(preview_id())) == Some(Preview::Focused)
}

/// Tokens and metrics for the context `ui` paints into.
pub(crate) fn kit(ui: &Ui) -> (Tokens, Metrics) {
    (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()))
}

pub(crate) use crate::tokens::{ink_on, mix};

/// A weight family the kit draws with.
#[derive(Clone, Copy)]
enum Weight {
    Medium,
    Semibold,
}

fn weights_id() -> Id {
    Id::unique("mtech.controls.weights")
}

/// Whether the medium and semibold families are installed, probed once per pass.
fn installed_weights(ctx: &Context) -> (bool, bool) {
    let pass = ctx.cumulative_pass_nr();
    if let Some((at, medium, semibold)) =
        ctx.data(|d| d.get_temp::<(u64, bool, bool)>(weights_id()))
        && at == pass
    {
        return (medium, semibold);
    }
    let (medium, semibold) = ctx.fonts(|f| {
        let families = &f.definitions().families;
        (
            families.contains_key(&crate::fonts::MEDIUM_FAMILY),
            families.contains_key(&crate::fonts::SEMIBOLD_FAMILY),
        )
    });
    ctx.data_mut(|d| d.insert_temp(weights_id(), (pass, medium, semibold)));
    (medium, semibold)
}

/// `font` in the `weight` family when it is proportional and that family is installed.
fn weighted(ui: &Ui, font: FontId, weight: Weight) -> FontId {
    if font.family != FontFamily::Proportional {
        return font;
    }
    let (medium, semibold) = installed_weights(ui.ctx());
    match weight {
        Weight::Medium if medium => FontId::new(font.size, crate::fonts::MEDIUM_FAMILY.clone()),
        Weight::Semibold if semibold => {
            FontId::new(font.size, crate::fonts::SEMIBOLD_FAMILY.clone())
        }
        _ => font,
    }
}

/// The theme's font for `style` at the medium weight.
pub(crate) fn medium_font(ui: &Ui, style: TextStyle) -> FontId {
    weighted(ui, style.resolve(ui.style()), Weight::Medium)
}

/// The theme's body face at `size` points and the medium weight.
pub(crate) fn medium_font_sized(ui: &Ui, size: f32) -> FontId {
    let body = TextStyle::Body.resolve(ui.style());
    weighted(ui, FontId::new(size, body.family), Weight::Medium)
}

/// The theme's body face at `size` points and the semibold weight.
pub(crate) fn semibold_font_sized(ui: &Ui, size: f32) -> FontId {
    let body = TextStyle::Body.resolve(ui.style());
    weighted(ui, FontId::new(size, body.family), Weight::Semibold)
}

/// Fill plus a 1 px inside stroke when `stroke` is visible.
pub(crate) fn paint_box(ui: &Ui, rect: Rect, radius: u8, fill: Color32, stroke: Color32) {
    let stroke = if stroke.a() == 0 {
        Stroke::NONE
    } else {
        Stroke::new(1.0, stroke)
    };
    ui.painter()
        .rect(rect, radius, fill, stroke, StrokeKind::Inside);
}

/// Two-pixel primary ring one pixel outside `rect`.
pub(crate) fn focus_ring(ui: &Ui, rect: Rect, radius: u8, t: &Tokens) {
    ui.painter().rect_stroke(
        rect.expand(1.0),
        CornerRadius::same(radius.saturating_add(1)),
        Stroke::new(2.0, t.primary),
        StrokeKind::Outside,
    );
}

/// One-row galley whose colour comes from the ink it is painted with.
pub(crate) fn text_galley(ui: &Ui, text: &str, font: FontId) -> Arc<Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
}

/// `text` on one row no wider than `max_w`, cut with '…' when it does not fit.
pub(crate) fn elided(ui: &Ui, text: &str, font: FontId, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, Color32::PLACEHOLDER);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// Width a full-row control fills; zero during a sizing pass.
pub(crate) fn fill_width(ui: &Ui) -> f32 {
    if ui.is_sizing_pass() {
        0.0
    } else {
        ui.available_width()
    }
}

/// Width rounded up to whole points.
pub(crate) fn whole(width: f32) -> f32 {
    width.ceil()
}

#[cfg(test)]
mod tests;

//! Semantic color accessors backed by the active egui theme.
//!
//! Callers should prefer `theme::error(ui)` over `Color32::from_rgb(255, 0, 0)`
//! so that all UI chrome follows the user-selected preset (Carl Dark, Tokyo
//! Night, Rerun MTech, custom upload, etc.) without per-site hardcoding.
//!
//! Most accessors read straight from `ui.visuals()`. `success()` and
//! `accent_secondary()` have no native `egui::Visuals` slot, so they're
//! stashed in `ctx.data_mut()` when a preset applies and fall back to
//! sensible defaults otherwise.

use eframe::egui::{Color32, Context, Id, Ui};

const SUCCESS_KEY: &str = "mtech.theme.success_color";
const ACCENT2_KEY: &str = "mtech.theme.accent_secondary";

fn success_id() -> Id { Id::new(SUCCESS_KEY) }
fn accent2_id() -> Id { Id::new(ACCENT2_KEY) }

pub fn set_success_color(ctx: &Context, c: Color32) {
    ctx.data_mut(|d| d.insert_temp(success_id(), c));
}

pub fn set_accent_secondary(ctx: &Context, c: Color32) {
    ctx.data_mut(|d| d.insert_temp(accent2_id(), c));
}

pub fn error(ui: &Ui) -> Color32 {
    ui.visuals().error_fg_color
}

pub fn warn(ui: &Ui) -> Color32 {
    ui.visuals().warn_fg_color
}

pub fn info(ui: &Ui) -> Color32 {
    ui.visuals().hyperlink_color
}

pub fn success(ui: &Ui) -> Color32 {
    success_ctx(ui.ctx())
}

pub fn success_ctx(ctx: &Context) -> Color32 {
    ctx.data(|d| d.get_temp::<Color32>(success_id()))
        .unwrap_or(Color32::from_rgb(72, 199, 142))
}

pub fn accent(ui: &Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

pub fn accent_secondary(ui: &Ui) -> Color32 {
    accent_secondary_ctx(ui.ctx())
}

pub fn accent_secondary_ctx(ctx: &Context) -> Color32 {
    ctx.data(|d| d.get_temp::<Color32>(accent2_id()))
        .unwrap_or(Color32::from_rgb(191, 33, 101))
}

pub fn strong_text(ui: &Ui) -> Color32 {
    ui.visuals().strong_text_color()
}

pub fn weak_text(ui: &Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

pub fn border(ui: &Ui) -> Color32 {
    ui.visuals().window_stroke.color
}

pub fn bg_surface(ui: &Ui) -> Color32 {
    ui.visuals().panel_fill
}

pub fn bg_faint(ui: &Ui) -> Color32 {
    ui.visuals().faint_bg_color
}

pub fn bg_extreme(ui: &Ui) -> Color32 {
    ui.visuals().extreme_bg_color
}

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

/// Live chart series colors for resource monitor / stress lab plots.
pub struct ChartPalette {
    pub avg_cpu: Color32,
    pub peak_cpu: Color32,
    pub clock: Color32,
    pub memory: Color32,
    pub page_file: Color32,
    pub disk: Color32,
    pub network: Color32,
    pub process_count: Color32,
    pub throughput: Color32,
    pub temperature: Color32,
    pub whea: Color32,
}

pub fn chart_palette(ui: &Ui) -> ChartPalette {
    ChartPalette {
        avg_cpu: info(ui),
        peak_cpu: error(ui),
        clock: success(ui),
        memory: warn(ui),
        page_file: accent_secondary(ui),
        disk: accent(ui),
        network: accent_secondary_ctx(ui.ctx()).gamma_multiply(0.85),
        process_count: weak_text(ui),
        throughput: info(ui),
        temperature: warn(ui),
        whea: accent_secondary(ui),
    }
}

pub fn usage_level(ui: &Ui, pct: f32) -> Color32 {
    if pct >= 90.0 {
        error(ui)
    } else if pct >= 70.0 {
        warn(ui)
    } else {
        success(ui)
    }
}

pub fn temp_level(ui: &Ui, temp_c: f32) -> Color32 {
    if temp_c >= 90.0 {
        error(ui)
    } else if temp_c >= 75.0 {
        warn(ui)
    } else {
        success(ui)
    }
}

/// Distinct series colors for per-core CPU charts (stable by index).
pub fn core_series_color(index: usize) -> Color32 {
    const PALETTE: [Color32; 16] = [
        Color32::from_rgb(120, 200, 255),
        Color32::from_rgb(220, 100, 100),
        Color32::from_rgb(170, 230, 140),
        Color32::from_rgb(220, 170, 90),
        Color32::from_rgb(200, 120, 220),
        Color32::from_rgb(140, 200, 200),
        Color32::from_rgb(235, 12, 38),
        Color32::from_rgb(12, 235, 97),
        Color32::from_rgb(240, 141, 55),
        Color32::from_rgb(0, 255, 255),
        Color32::from_rgb(255, 0, 255),
        Color32::from_rgb(128, 0, 128),
        Color32::from_rgb(255, 180, 100),
        Color32::from_rgb(100, 180, 255),
        Color32::from_rgb(180, 255, 120),
        Color32::from_rgb(255, 120, 180),
    ];
    PALETTE[index % PALETTE.len()]
}

pub fn result_pass(ui: &Ui) -> Color32 {
    success(ui)
}

pub fn result_fail(ui: &Ui) -> Color32 {
    error(ui)
}

pub fn result_aborted(ui: &Ui) -> Color32 {
    warn(ui)
}

pub fn result_unknown(ui: &Ui) -> Color32 {
    weak_text(ui)
}

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

/// Body text in the current widget state; brightens on hover and press on themes that leave
/// `override_text_color` unset.
pub fn text(ui: &Ui) -> Color32 {
    ui.visuals().text_color()
}

pub fn weak_text(ui: &Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

/// Dimmer than [`weak_text`], for placeholders and inert separators.
pub fn faint_text(ui: &Ui) -> Color32 {
    weak_text(ui).gamma_multiply(0.65)
}

/// Number of distinct tag accents [`tag_color`] cycles through.
pub const TAG_LEN: usize = 5;

/// Theme-following accent for a categorical tag whose only job is to differ from its neighbours —
/// a registry value type, a hive, a transport kind. Wraps at [`TAG_LEN`].
///
/// Not [`series_color`]: that palette is CVD-validated and deliberately fixed, because a chart's
/// colors have to stay stable across themes and screenshots. Tags have no such contract.
pub fn tag_color(ui: &Ui, index: usize) -> Color32 {
    match index % TAG_LEN {
        0 => info(ui),
        1 => success(ui),
        2 => warn(ui),
        3 => accent_secondary(ui),
        _ => strong_text(ui),
    }
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
    status_style(ui, temp_status(temp_c)).color
}

/// Distinct series colors for per-core CPU charts (stable by index).
pub fn core_series_color(index: usize) -> Color32 {
    // Fails CVD validation past 8 slots; new charts use series_color instead.
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

/// A run that neither certifies nor condemns: it needs operator review, so it
/// reads as a warning rather than as an unrecognised state.
pub fn result_inconclusive(ui: &Ui) -> Color32 {
    warn(ui)
}

pub fn result_unknown(ui: &Ui) -> Color32 {
    weak_text(ui)
}

/// Number of distinct series the validated categorical palette supports.
pub const SERIES_LEN: usize = 6;

/// Categorical series colors; the order is CVD-validated and must not be reordered or extended.
pub const SERIES_PALETTE: [Color32; SERIES_LEN] = [
    Color32::from_rgb(0x41, 0x84, 0xE4),
    Color32::from_rgb(0xE5, 0x53, 0x4B),
    Color32::from_rgb(0x1F, 0x9D, 0xA6),
    Color32::from_rgb(0xCC, 0x6B, 0x2C),
    Color32::from_rgb(0x82, 0x56, 0xD0),
    Color32::from_rgb(0x46, 0x95, 0x4A),
];

/// Color for series `index`, or `None` past `SERIES_LEN`; fold extra series into one `other_series` bucket.
pub fn series_color(index: usize) -> Option<Color32> {
    SERIES_PALETTE.get(index).copied()
}

/// Neutral color for the folded "Other" bucket that absorbs series past `SERIES_LEN`.
pub fn other_series(ui: &Ui) -> Color32 {
    weak_text(ui)
}

// Violet ramp, hue ~259deg, every channel strictly decreasing.
const SEQUENTIAL_VIOLET: [Color32; 5] = [
    Color32::from_rgb(0xED, 0xE7, 0xFA),
    Color32::from_rgb(0xC4, 0xB2, 0xF0),
    Color32::from_rgb(0x9B, 0x7B, 0xE3),
    Color32::from_rgb(0x70, 0x48, 0xC4),
    Color32::from_rgb(0x43, 0x25, 0x7A),
];

/// Maps `t` in 0.0..=1.0 onto the violet magnitude ramp, pale at 0.0 to deep at 1.0.
pub fn sequential(t: f32) -> Color32 {
    sample_ramp(&SEQUENTIAL_VIOLET, t)
}

/// Violet magnitude ramp with the direction flipped on dark themes so higher `t` gains contrast.
pub fn sequential_contrast(ui: &Ui, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if ui.visuals().dark_mode {
        sequential(1.0 - t)
    } else {
        sequential(t)
    }
}

fn sample_ramp(stops: &[Color32], t: f32) -> Color32 {
    let last = stops.len().saturating_sub(1);
    let scaled = t.clamp(0.0, 1.0) * last as f32;
    let i = (scaled.floor() as usize).min(last.saturating_sub(1));
    match (stops.get(i), stops.get(i + 1)) {
        (Some(a), Some(b)) => lerp_rgb(*a, *b, scaled - i as f32),
        _ => stops.first().copied().unwrap_or(Color32::TRANSPARENT),
    }
}

fn lerp_rgb(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Four-level status scale for charts, tiles and meters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Good,
    Warn,
    Serious,
    Critical,
}

impl Status {
    /// Short label token for this status.
    pub fn label(self) -> &'static str {
        match self {
            Status::Good => "OK",
            Status::Warn => "Warn",
            Status::Serious => "Serious",
            Status::Critical => "Critical",
        }
    }
}

/// Status color paired with its label token.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct StatusStyle {
    pub color: Color32,
    pub label: &'static str,
}

/// Color and label token for `status`, drawn from the theme's success/warn/error slots.
pub fn status_style(ui: &Ui, status: Status) -> StatusStyle {
    let color = match status {
        Status::Good => success(ui),
        Status::Warn => warn(ui),
        Status::Serious => lerp_rgb(warn(ui), error(ui), 0.5),
        Status::Critical => error(ui),
    };
    StatusStyle { color, label: status.label() }
}

/// Status for a 0..100 usage percentage, using the same thresholds as `usage_level`.
pub fn usage_status(pct: f32) -> Status {
    if pct >= 90.0 {
        Status::Critical
    } else if pct >= 70.0 {
        Status::Warn
    } else {
        Status::Good
    }
}

/// Status for a Celsius temperature; the single source of the 75/90 thresholds `temp_level` paints.
pub fn temp_status(temp_c: f32) -> Status {
    if temp_c >= 90.0 {
        Status::Critical
    } else if temp_c >= 75.0 {
        Status::Warn
    } else {
        Status::Good
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_series_palette_keeps_its_validated_order() {
        let expected = [
            (0x41, 0x84, 0xE4),
            (0xE5, 0x53, 0x4B),
            (0x1F, 0x9D, 0xA6),
            (0xCC, 0x6B, 0x2C),
            (0x82, 0x56, 0xD0),
            (0x46, 0x95, 0x4A),
        ];
        assert_eq!(SERIES_PALETTE.len(), SERIES_LEN);
        for (slot, (r, g, b)) in expected.iter().enumerate() {
            let c = series_color(slot).expect("slot inside the palette");
            assert_eq!((c.r(), c.g(), c.b()), (*r, *g, *b), "slot {slot} moved");
        }
    }

    #[test]
    fn a_series_past_the_palette_gets_no_colour_of_its_own() {
        assert!(series_color(SERIES_LEN).is_none());
        assert!(series_color(SERIES_LEN + 7).is_none());
    }

    #[test]
    fn the_sequential_ramp_never_brightens() {
        let steps = 64;
        let mut prev = sequential(0.0);
        for i in 1..=steps {
            let c = sequential(i as f32 / steps as f32);
            assert!(
                c.r() <= prev.r() && c.g() <= prev.g() && c.b() <= prev.b(),
                "channel rose between step {} and {i}",
                i - 1
            );
            prev = c;
        }
        let (pale, deep) = (sequential(0.0), sequential(1.0));
        assert!(deep.r() < pale.r() && deep.g() < pale.g() && deep.b() < pale.b());
    }

    #[test]
    fn usage_and_temp_grade_on_their_documented_thresholds() {
        assert_eq!(usage_status(69.9), Status::Good);
        assert_eq!(usage_status(70.0), Status::Warn);
        assert_eq!(usage_status(89.9), Status::Warn);
        assert_eq!(usage_status(90.0), Status::Critical);
        assert_eq!(temp_status(74.9), Status::Good);
        assert_eq!(temp_status(75.0), Status::Warn);
        assert_eq!(temp_status(89.9), Status::Warn);
        assert_eq!(temp_status(90.0), Status::Critical);
    }
}

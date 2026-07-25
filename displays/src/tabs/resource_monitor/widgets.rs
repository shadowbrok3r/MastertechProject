//! Dashboard widget vocabulary for the resource monitor: circular gauges, linear
//! meters, stat tiles, status pills and sparklines.
//!
//! Every widget takes an explicit desired size so callers can lay them out in a
//! grid, and every value arrives as a [`Reading`], which has no zero to fall back
//! on when a sensor was never read.

use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use crate::ui_tools::{icons, theme};

/// Painted in place of a value that was never measured.
pub const ABSENT_TEXT: &str = "—";

pub const GAUGE_SIZE: Vec2 = vec2(104.0, 118.0);
pub const METER_SIZE: Vec2 = vec2(260.0, 22.0);
pub const TILE_SIZE: Vec2 = vec2(158.0, 86.0);
pub const PILL_SIZE: Vec2 = vec2(150.0, 22.0);
pub const SPARKLINE_SIZE: Vec2 = vec2(120.0, 20.0);

const GAUGE_START_DEG: f32 = 135.0;
const GAUGE_SWEEP_DEG: f32 = 270.0;

/// Why a reading has no value. `NoSensor` covers `CpuTempSource::None` and a rail
/// label missing from `TelemetrySnapshot::rails`; `NotSampled` and `Unavailable`
/// map onto the same-named `WheaStatus` variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Absent {
    #[default]
    NoSensor,
    NotSampled,
    Unavailable,
}

impl Absent {
    /// Short reason text; never a number.
    pub fn note(self) -> &'static str {
        match self {
            Self::NoSensor => "no sensor",
            Self::NotSampled => "not measured",
            Self::Unavailable => "unavailable",
        }
    }
}

/// A sensor value that may never have been read. Absence is its own variant, so a
/// caller cannot hand a widget `0.0` for a quantity nothing reported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reading {
    Measured(f32),
    Absent(Absent),
}

impl Default for Reading {
    fn default() -> Self {
        Self::NO_SENSOR
    }
}

impl Reading {
    pub const NO_SENSOR: Self = Self::Absent(Absent::NoSensor);
    pub const NOT_SAMPLED: Self = Self::Absent(Absent::NotSampled);
    pub const UNAVAILABLE: Self = Self::Absent(Absent::Unavailable);

    /// `Some` measures; `None` is absent for `reason`.
    pub fn new(value: Option<f32>, reason: Absent) -> Self {
        value.map_or(Self::Absent(reason), Self::Measured)
    }

    /// The value, only when one was measured and it is finite.
    pub fn finite(self) -> Option<f32> {
        match self {
            Self::Measured(v) if v.is_finite() => Some(v),
            _ => None,
        }
    }

    pub fn is_absent(self) -> bool {
        self.finite().is_none()
    }

    /// Reason text for a value that cannot be shown.
    pub fn absent_note(self) -> &'static str {
        match self {
            Self::Absent(reason) => reason.note(),
            Self::Measured(_) => "no reading",
        }
    }
}

impl From<Option<f32>> for Reading {
    fn from(value: Option<f32>) -> Self {
        Self::new(value, Absent::NoSensor)
    }
}

impl From<f32> for Reading {
    fn from(value: f32) -> Self {
        Self::Measured(value)
    }
}

/// `theme::Status`'s reserved tiers plus the ungraded state the theme scale has no
/// room for. `NotMeasured` is the default, so a status nothing graded cannot read
/// as good.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    Good,
    Warn,
    Serious,
    Critical,
    #[default]
    NotMeasured,
}

impl From<theme::Status> for Status {
    fn from(tier: theme::Status) -> Self {
        match tier {
            theme::Status::Good => Self::Good,
            theme::Status::Warn => Self::Warn,
            theme::Status::Serious => Self::Serious,
            theme::Status::Critical => Self::Critical,
        }
    }
}

impl Status {
    /// Theme tier for a graded status; `None` when nothing was measured.
    pub fn tier(self) -> Option<theme::Status> {
        match self {
            Self::Good => Some(theme::Status::Good),
            Self::Warn => Some(theme::Status::Warn),
            Self::Serious => Some(theme::Status::Serious),
            Self::Critical => Some(theme::Status::Critical),
            Self::NotMeasured => None,
        }
    }

    pub fn color(self, ui: &Ui) -> Color32 {
        self.tier().map_or_else(
            || theme::weak_text(ui),
            |tier| theme::status_style(ui, tier).color,
        )
    }

    /// Glyph paired with [`Self::color`]; each tier has its own shape.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Good => icons::STATUS_ON,
            Self::Warn => icons::STATUS_WARN,
            Self::Serious => icons::STATUS_ERR,
            Self::Critical => icons::CRITICAL,
            Self::NotMeasured => icons::STATUS_OFF,
        }
    }

    pub fn label(self) -> &'static str {
        self.tier().map_or("Not measured", theme::Status::label)
    }

    pub fn is_measured(self) -> bool {
        !matches!(self, Self::NotMeasured)
    }

    /// Grades a percentage through `theme::usage_status`.
    pub fn from_usage_pct(value: Reading) -> Self {
        value
            .finite()
            .map_or(Self::NotMeasured, |pct| theme::usage_status(pct).into())
    }

    /// Grades Celsius through `theme::temp_status`.
    pub fn from_temp_c(value: Reading) -> Self {
        value
            .finite()
            .map_or(Self::NotMeasured, |c| theme::temp_status(c).into())
    }
}

/// How a mark takes its colour from the value it shows.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Ramp {
    /// `theme::usage_status` on the value's fraction of the range.
    #[default]
    Usage,
    /// `theme::temp_status` on the value in Celsius.
    TempC,
    /// Categorical identity for series `index`; past `theme::SERIES_LEN` it folds
    /// into the neutral Other colour.
    Series(usize),
    Fixed(Color32),
    /// No level encoding.
    Neutral,
}

impl Ramp {
    fn color(self, ui: &Ui, value: f32, frac: f32) -> Color32 {
        match self {
            Self::Usage => theme::status_style(ui, theme::usage_status(frac * 100.0)).color,
            Self::TempC => theme::status_style(ui, theme::temp_status(value)).color,
            Self::Series(index) => {
                theme::series_color(index).unwrap_or_else(|| theme::other_series(ui))
            }
            Self::Fixed(color) => color,
            Self::Neutral => theme::accent(ui),
        }
    }
}

/// A bounded value as an arc ring, with the value centred and the label beneath.
pub struct Gauge<'a> {
    pub label: &'a str,
    pub value: Reading,
    pub unit: &'a str,
    pub decimals: usize,
    pub range: (f32, f32),
    pub ramp: Ramp,
}

impl Default for Gauge<'_> {
    fn default() -> Self {
        Self {
            label: "",
            value: Reading::default(),
            unit: "%",
            decimals: 0,
            range: (0.0, 100.0),
            ramp: Ramp::default(),
        }
    }
}

/// A labelled horizontal bar with the value right-aligned. Length reads more
/// accurately than angle, so this is the form for values compared across items.
pub struct Meter<'a> {
    pub label: &'a str,
    pub value: Reading,
    pub unit: &'a str,
    pub decimals: usize,
    pub range: (f32, f32),
    pub ramp: Ramp,
    /// Replaces the formatted value in the right-hand column.
    pub value_text: Option<&'a str>,
}

impl Default for Meter<'_> {
    fn default() -> Self {
        Self {
            label: "",
            value: Reading::default(),
            unit: "%",
            decimals: 1,
            range: (0.0, 100.0),
            ramp: Ramp::default(),
            value_text: None,
        }
    }
}

/// A compact KPI: caption, large value, unit, and an optional sub-line.
#[derive(Default)]
pub struct StatTile<'a> {
    pub caption: &'a str,
    pub value: Reading,
    pub unit: &'a str,
    pub decimals: usize,
    /// Second line under the value; the absent reason replaces it when there is
    /// no value.
    pub sub: Option<&'a str>,
    /// Status mark in the top-right corner, icon and colour together.
    pub status: Option<Status>,
    /// Trend along the tile's lower edge; non-finite samples break the line.
    pub spark: Option<&'a [f32]>,
    /// Trend colour; pass `theme::series_color(i)` so a series past `SERIES_LEN`
    /// folds into the neutral Other colour.
    pub spark_color: Option<Color32>,
}

/// Status colour, icon and text label together, so identity is never colour alone.
#[derive(Default)]
pub struct StatusPill<'a> {
    pub status: Status,
    /// Falls back to [`Status::label`] when empty.
    pub label: &'a str,
}

/// A thin trend line. Non-finite samples break the line rather than plotting zero.
#[derive(Default)]
pub struct Sparkline<'a> {
    pub samples: &'a [f32],
    /// Takes `theme::series_color(i)` directly; `None` folds to the Other colour.
    pub color: Option<Color32>,
}

pub fn circular_gauge(ui: &mut Ui, size: Vec2, gauge: &Gauge<'_>) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let label_h = (rect.height() * 0.17).clamp(9.0, 16.0);
    let ring_box = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - label_h - 2.0));
    let diameter = ring_box.width().min(ring_box.height());
    let stroke_w = (diameter * 0.11).clamp(3.0, 9.0);
    let radius = diameter / 2.0 - stroke_w / 2.0 - 1.0;
    if radius <= 1.0 {
        return response;
    }

    let value = gauge.value.finite();
    let frac = value.map_or(0.0, |v| fraction(v, gauge.range));
    let center = ring_box.center();
    let track = theme::weak_text(ui).gamma_multiply(if value.is_some() { 0.30 } else { 0.20 });

    ui.painter().add(Shape::line(
        arc_points(center, radius, GAUGE_START_DEG, GAUGE_SWEEP_DEG),
        Stroke::new(stroke_w, track),
    ));

    if let Some(v) = value.filter(|_| frac > 0.0) {
        let fill = gauge.ramp.color(ui, v, frac);
        let points = arc_points(center, radius, GAUGE_START_DEG, GAUGE_SWEEP_DEG * frac);
        let painter = ui.painter();
        // Round caps on the filled arc's ends.
        if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
            painter.circle_filled(first, stroke_w / 2.0, fill);
            painter.circle_filled(last, stroke_w / 2.0, fill);
        }
        painter.add(Shape::line(points, Stroke::new(stroke_w, fill)));
    }

    let value_size = (diameter * 0.26).clamp(11.0, 26.0);
    match value {
        Some(v) => paint_value(
            ui,
            center,
            Align2::CENTER_CENTER,
            &format_value(v, gauge.decimals),
            gauge.unit,
            value_size,
            theme::strong_text(ui),
        ),
        None => {
            paint_value(
                ui,
                center - vec2(0.0, value_size * 0.20),
                Align2::CENTER_CENTER,
                ABSENT_TEXT,
                "",
                value_size,
                theme::weak_text(ui),
            );
            let note_size = (value_size * 0.42).clamp(7.0, 11.0);
            ui.painter().text(
                center + vec2(0.0, value_size * 0.50),
                Align2::CENTER_CENTER,
                gauge.value.absent_note(),
                FontId::proportional(note_size),
                theme::weak_text(ui),
            );
        }
    }

    let label_rect = Rect::from_min_max(pos2(rect.min.x, rect.max.y - label_h), rect.max);
    ui.painter().with_clip_rect(label_rect).text(
        label_rect.center(),
        Align2::CENTER_CENTER,
        gauge.label,
        FontId::proportional(label_h.min(13.0)),
        theme::weak_text(ui),
    );

    absent_hover(response, gauge.value)
}

pub fn linear_meter(ui: &mut Ui, size: Vec2, meter: &Meter<'_>) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    const GAP: f32 = 8.0;
    let text_size = (rect.height() * 0.60).clamp(9.0, 14.0);
    let label_w = rect.width() * 0.32;
    let value_w = rect.width() * 0.20;
    let track_w = (rect.width() - label_w - value_w - GAP * 2.0).max(0.0);

    let label_rect = Rect::from_min_size(rect.min, vec2(label_w, rect.height()));
    ui.painter().with_clip_rect(label_rect).text(
        pos2(label_rect.left(), label_rect.center().y),
        Align2::LEFT_CENTER,
        meter.label,
        FontId::proportional(text_size),
        theme::strong_text(ui),
    );

    let value = meter.value.finite();
    let track_h = (rect.height() * 0.42).clamp(5.0, 11.0);
    let track_rect = Rect::from_center_size(
        pos2(label_rect.right() + GAP + track_w / 2.0, rect.center().y),
        vec2(track_w, track_h),
    );
    let corner = track_h / 2.0;
    let track = theme::weak_text(ui).gamma_multiply(if value.is_some() { 0.22 } else { 0.14 });
    ui.painter().rect_filled(track_rect, corner, track);

    if let Some(v) = value {
        let frac = fraction(v, meter.range);
        if frac > 0.0 {
            // Nonzero fill keeps a minimum visible width.
            let filled_w = (track_w * frac).clamp(2.0_f32.min(track_w), track_w);
            let fill_rect = Rect::from_min_size(track_rect.min, vec2(filled_w, track_h));
            ui.painter()
                .rect_filled(fill_rect, corner, meter.ramp.color(ui, v, frac));
        }
    }

    let value_anchor = pos2(rect.right(), rect.center().y);
    match value {
        Some(v) => {
            let text = meter
                .value_text
                .map_or_else(|| format_value(v, meter.decimals), str::to_owned);
            paint_value(
                ui,
                value_anchor,
                Align2::RIGHT_CENTER,
                &text,
                meter.unit,
                text_size,
                theme::strong_text(ui),
            );
        }
        None => paint_value(
            ui,
            value_anchor,
            Align2::RIGHT_CENTER,
            ABSENT_TEXT,
            "",
            text_size,
            theme::weak_text(ui),
        ),
    }

    absent_hover(response, meter.value)
}

pub fn stat_tile(ui: &mut Ui, size: Vec2, tile: &StatTile<'_>) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    ui.painter().rect_filled(rect, 6.0, theme::bg_faint(ui));
    ui.painter().rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, theme::border(ui)),
        StrokeKind::Inside,
    );

    let inner = rect.shrink((size.y * 0.10).clamp(5.0, 9.0));
    let caption_size = (inner.height() * 0.17).clamp(8.0, 12.0);
    let value_size = (inner.height() * 0.36).clamp(13.0, 24.0);
    let sub_size = (inner.height() * 0.16).clamp(8.0, 11.0);

    let caption_rect =
        Rect::from_min_size(inner.min, vec2(inner.width() * 0.78, caption_size + 2.0));
    ui.painter().with_clip_rect(caption_rect).text(
        caption_rect.left_top(),
        Align2::LEFT_TOP,
        tile.caption,
        FontId::proportional(caption_size),
        theme::weak_text(ui),
    );

    if let Some(status) = tile.status {
        let icon_size = (inner.height() * 0.24).clamp(10.0, 15.0);
        ui.painter().text(
            inner.right_top(),
            Align2::RIGHT_TOP,
            status.icon(),
            FontId::proportional(icon_size),
            status.color(ui),
        );
    }

    let value = tile.value.finite();
    let value_pos = pos2(inner.left(), inner.top() + caption_size + 4.0);
    match value {
        Some(v) => paint_value(
            ui,
            value_pos,
            Align2::LEFT_TOP,
            &format_value(v, tile.decimals),
            tile.unit,
            value_size,
            theme::strong_text(ui),
        ),
        None => paint_value(
            ui,
            value_pos,
            Align2::LEFT_TOP,
            ABSENT_TEXT,
            "",
            value_size,
            theme::weak_text(ui),
        ),
    }

    let sub = match value {
        Some(_) => tile.sub.unwrap_or_default(),
        None => tile.value.absent_note(),
    };
    let mut band_top = value_pos.y + value_size * 1.2;
    if !sub.is_empty() && inner.bottom() - band_top >= sub_size {
        let sub_bottom = (band_top + sub_size + 2.0).min(inner.bottom());
        let sub_rect = Rect::from_min_max(
            pos2(inner.left(), band_top),
            pos2(inner.right(), sub_bottom),
        );
        ui.painter().with_clip_rect(sub_rect).text(
            sub_rect.left_top(),
            Align2::LEFT_TOP,
            sub,
            FontId::proportional(sub_size),
            theme::weak_text(ui),
        );
        band_top = sub_bottom + 2.0;
    }

    if let Some(samples) = tile.spark {
        let spark_rect = Rect::from_min_max(pos2(inner.left(), band_top), inner.max);
        if spark_rect.height() >= 6.0 {
            let color = tile.spark_color.unwrap_or_else(|| theme::other_series(ui));
            paint_sparkline(ui, spark_rect, samples, color);
        }
    }

    absent_hover(response, tile.value)
}

pub fn status_pill(ui: &mut Ui, size: Vec2, pill: &StatusPill<'_>) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let color = pill.status.color(ui);
    let corner = rect.height() / 2.0;
    ui.painter()
        .rect_filled(rect, corner, color.gamma_multiply(0.16));
    ui.painter().rect_stroke(
        rect,
        corner,
        Stroke::new(1.0, color.gamma_multiply(0.55)),
        StrokeKind::Inside,
    );

    let icon_size = (rect.height() * 0.62).clamp(10.0, 16.0);
    let text_size = (rect.height() * 0.55).clamp(9.0, 14.0);
    let icon_left = rect.left() + corner * 0.9;
    ui.painter().text(
        pos2(icon_left, rect.center().y),
        Align2::LEFT_CENTER,
        pill.status.icon(),
        FontId::proportional(icon_size),
        color,
    );

    let label = if pill.label.is_empty() {
        pill.status.label()
    } else {
        pill.label
    };
    let text_rect = Rect::from_min_max(pos2(icon_left + icon_size + 5.0, rect.top()), rect.max);
    let ink = if pill.status.is_measured() {
        theme::strong_text(ui)
    } else {
        theme::weak_text(ui)
    };
    ui.painter().with_clip_rect(text_rect).text(
        pos2(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(text_size),
        ink,
    );

    response
}

pub fn sparkline(ui: &mut Ui, size: Vec2, spark: &Sparkline<'_>) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let color = spark.color.unwrap_or_else(|| theme::other_series(ui));
    paint_sparkline(ui, rect, spark.samples, color);
    response
}

/// Paints `value` in monospace with a smaller trailing `unit` in weak text.
fn paint_value(
    ui: &Ui,
    pos: Pos2,
    anchor: Align2,
    value: &str,
    unit: &str,
    size: f32,
    ink: Color32,
) {
    let painter = ui.painter();
    let value_galley = painter.layout_no_wrap(value.to_owned(), FontId::monospace(size), ink);
    let value_size = value_galley.size();

    let unit_size = (size * 0.50).clamp(8.0, 13.0);
    let unit_galley = (!unit.is_empty()).then(|| {
        painter.layout_no_wrap(
            unit.to_owned(),
            FontId::proportional(unit_size),
            theme::weak_text(ui),
        )
    });
    let gap = if unit_galley.is_some() {
        size * 0.14
    } else {
        0.0
    };
    let total = vec2(
        value_size.x + gap + unit_galley.as_ref().map_or(0.0, |g| g.size().x),
        value_size.y,
    );

    let rect = anchor.anchor_size(pos, total);
    painter.galley(rect.min, value_galley, ink);
    if let Some(galley) = unit_galley {
        let unit_pos = pos2(
            rect.min.x + value_size.x + gap,
            rect.max.y - galley.size().y - size * 0.06,
        );
        painter.galley(unit_pos, galley, theme::weak_text(ui));
    }
}

fn paint_sparkline(ui: &Ui, rect: Rect, samples: &[f32], color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.2, color);

    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    let mut finite = 0usize;
    for &v in samples.iter().filter(|v| v.is_finite()) {
        lo = lo.min(v);
        hi = hi.max(v);
        finite += 1;
    }
    if finite == 0 {
        // Dim baseline stands in for a trend with no samples.
        painter.hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1.0, theme::weak_text(ui).gamma_multiply(0.25)),
        );
        return;
    }

    let span = (hi - lo).max(f32::EPSILON);
    let steps = samples.len().saturating_sub(1).max(1) as f32;
    let dx = rect.width() / steps;
    let mut run: Vec<Pos2> = Vec::new();
    for (i, &v) in samples.iter().enumerate() {
        if v.is_finite() {
            let y = rect.bottom() - ((v - lo) / span) * rect.height();
            run.push(pos2(rect.left() + dx * i as f32, y));
        } else {
            flush_run(painter, &mut run, stroke);
        }
    }
    flush_run(painter, &mut run, stroke);
}

/// Draws the pending run as a polyline, or as a dot when it holds one point.
fn flush_run(painter: &Painter, run: &mut Vec<Pos2>, stroke: Stroke) {
    if run.len() >= 2 {
        painter.add(Shape::line(std::mem::take(run), stroke));
    } else if let Some(&point) = run.first() {
        painter.circle_filled(point, stroke.width, stroke.color);
    }
    run.clear();
}

/// Attaches the absent reason as hover text when there is no value.
fn absent_hover(response: Response, value: Reading) -> Response {
    if value.is_absent() {
        response.on_hover_text(value.absent_note())
    } else {
        response
    }
}

fn fraction(value: f32, range: (f32, f32)) -> f32 {
    let (min, max) = range;
    if (max - min).abs() <= f32::EPSILON {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

fn format_value(value: f32, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

fn arc_points(center: Pos2, radius: f32, start_deg: f32, sweep_deg: f32) -> Vec<Pos2> {
    let steps = ((sweep_deg.abs() / 4.0).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let deg = start_deg + sweep_deg * (i as f32 / steps as f32);
            center + Vec2::angled(deg.to_radians()) * radius
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_value_is_absent_not_zero() {
        let absent = Reading::from(None);
        assert_eq!(absent, Reading::NO_SENSOR);
        assert_eq!(absent.finite(), None);
        assert!(absent.is_absent());
        assert_eq!(Reading::default(), Reading::NO_SENSOR);
    }

    #[test]
    fn a_non_finite_measurement_never_renders_as_a_number() {
        assert_eq!(Reading::Measured(f32::NAN).finite(), None);
        assert_eq!(Reading::Measured(f32::INFINITY).finite(), None);
        assert!(Reading::Measured(f32::NAN).is_absent());
        assert_eq!(Reading::Measured(0.0).finite(), Some(0.0));
    }

    #[test]
    fn absence_carries_its_reason() {
        assert_eq!(Reading::NO_SENSOR.absent_note(), "no sensor");
        assert_eq!(Reading::NOT_SAMPLED.absent_note(), "not measured");
        assert_eq!(Reading::UNAVAILABLE.absent_note(), "unavailable");
        assert_eq!(
            Reading::new(None, Absent::Unavailable),
            Reading::UNAVAILABLE
        );
    }

    #[test]
    fn an_ungraded_status_is_never_good() {
        assert_eq!(Status::default(), Status::NotMeasured);
        assert!(!Status::default().is_measured());
        assert_eq!(Status::default().tier(), None);
        assert_eq!(Status::default().label(), "Not measured");
        assert_eq!(
            Status::from_usage_pct(Reading::NO_SENSOR),
            Status::NotMeasured
        );
        assert_eq!(Status::from_temp_c(Reading::default()), Status::NotMeasured);
        assert_eq!(
            Status::from_temp_c(Reading::Measured(f32::NAN)),
            Status::NotMeasured
        );
    }

    #[test]
    fn status_thresholds_match_the_theme_ramps() {
        assert_eq!(Status::from_usage_pct(12.0.into()), Status::Good);
        assert_eq!(Status::from_usage_pct(70.0.into()), Status::Warn);
        assert_eq!(Status::from_usage_pct(90.0.into()), Status::Critical);
        assert_eq!(Status::from_temp_c(60.0.into()), Status::Good);
        assert_eq!(Status::from_temp_c(75.0.into()), Status::Warn);
        assert_eq!(Status::from_temp_c(90.0.into()), Status::Critical);
    }

    #[test]
    fn every_theme_tier_maps_to_a_widget_tier() {
        for tier in [
            theme::Status::Good,
            theme::Status::Warn,
            theme::Status::Serious,
            theme::Status::Critical,
        ] {
            let status = Status::from(tier);
            assert_eq!(status.tier(), Some(tier));
            assert_eq!(status.label(), tier.label());
            assert!(status.is_measured());
        }
    }

    #[test]
    fn every_status_tier_has_its_own_glyph() {
        let tiers = [
            Status::Good,
            Status::Warn,
            Status::Serious,
            Status::Critical,
            Status::NotMeasured,
        ];
        for (i, a) in tiers.iter().enumerate() {
            for b in &tiers[i + 1..] {
                assert_ne!(a.icon(), b.icon());
            }
        }
    }

    #[test]
    fn a_series_past_the_palette_has_no_colour_of_its_own() {
        assert!(theme::series_color(theme::SERIES_LEN - 1).is_some());
        assert!(theme::series_color(theme::SERIES_LEN).is_none());
    }

    #[test]
    fn a_fraction_stays_inside_the_ring() {
        assert_eq!(fraction(50.0, (0.0, 100.0)), 0.5);
        assert_eq!(fraction(-10.0, (0.0, 100.0)), 0.0);
        assert_eq!(fraction(400.0, (0.0, 100.0)), 1.0);
        assert!((fraction(11.9, (11.4, 12.6)) - 0.4167).abs() < 0.001);
        assert_eq!(fraction(5.0, (5.0, 5.0)), 0.0);
    }

    #[test]
    fn the_arc_spans_the_requested_sweep() {
        let points = arc_points(Pos2::ZERO, 10.0, GAUGE_START_DEG, GAUGE_SWEEP_DEG);
        assert!(points.len() >= 68);
        let full = arc_points(Pos2::ZERO, 10.0, 0.0, 360.0);
        let first = full.first().copied().expect("arc has points");
        let last = full.last().copied().expect("arc has points");
        assert!((first - last).length() < 0.01);
    }
}

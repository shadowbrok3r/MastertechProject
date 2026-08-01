//! Card, badge, prose, and date helpers for the admin-console intel views.

use chrono::{DateTime, Utc};
use eframe::egui::{
    pos2,
    text::{LayoutJob, TextWrapping},
    Align, Color32, CornerRadius, Id, Label, Layout, Response, RichText, Sense, Stroke, StrokeKind,
    TextFormat, TextStyle, TextWrapMode, Ui, Vec2, WidgetText,
};

use crate::ui_tools::{icons, theme};

const DATE_FMT: &str = "%m/%d/%Y";
const DATE_TIME_FMT: &str = "%m/%d/%Y %H:%M";
const KV_LABEL_WIDTH: f32 = 108.0;
const BADGE_CORNER_RADIUS: u8 = 4;
const BADGE_PAD_X: f32 = 6.0;
const BADGE_PAD_Y: f32 = 1.0;
const BADGE_FILL_ALPHA: f32 = 0.16;

/// Tool-call markup fragments that mark a leaked AI draft.
const MARKUP_MARKERS: [&str; 4] = [
    "<parameter name=",
    "</parameter>",
    "<invoke name=",
    "<function_calls>",
];

pub fn section_card(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    subtitle: Option<&str>,
    body: impl FnOnce(&mut Ui),
) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.colored_label(theme::accent(ui), icon);
            ui.label(RichText::new(title).strong().color(theme::strong_text(ui)));
            if let Some(subtitle) = subtitle {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(subtitle).small().color(theme::weak_text(ui)));
                });
            }
        });
        ui.separator();
        body(ui);
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
}

/// Allocates its full size before painting, so a wrapped layout breaks the row
/// ahead of the chip instead of squeezing it into the leftover width.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = WidgetText::from(RichText::new(text).small().monospace().color(color))
        .into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Small);
    let pad = Vec2::new(BADGE_PAD_X, BADGE_PAD_Y);
    let (rect, response) = ui.allocate_at_least(galley.size() + pad * 2.0, Sense::hover());
    if ui.is_rect_visible(rect) {
        let radius = CornerRadius::same(BADGE_CORNER_RADIUS);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, color.gamma_multiply(BADGE_FILL_ALPHA));
        painter.rect_stroke(rect, radius, Stroke::new(1.0, color), StrokeKind::Inside);
        let text_pos = pos2(rect.left() + pad.x, rect.center().y - galley.size().y * 0.5);
        painter.galley(text_pos, galley, color);
    }
    response
}

pub fn kv_row(ui: &mut Ui, label: &str, value: &str) {
    let weak = theme::weak_text(ui);
    let strong = theme::strong_text(ui);
    ui.horizontal_top(|ui| {
        let size = Vec2::new(KV_LABEL_WIDTH, ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::TOP), |ui| {
            ui.add(Label::new(RichText::new(label).small().color(weak)).truncate());
        });
        ui.add(Label::new(RichText::new(value).color(strong)).wrap());
    });
}

pub fn wrapped_text(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(Label::new(RichText::new(text).color(color)).wrap());
}

/// Prose capped at `collapsed_lines` rows with a more/less toggle keyed on `id_salt`.
pub fn expandable_text(ui: &mut Ui, id_salt: &str, text: &str, collapsed_lines: usize) {
    if text.trim().is_empty() {
        return;
    }
    let state_id = Id::new(("mtech.expandable_text", id_salt));
    let open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(state_id))
        .unwrap_or(false);
    let font_id = ui
        .style()
        .text_styles
        .get(&TextStyle::Body)
        .cloned()
        .unwrap_or_default();
    let color = ui.visuals().text_color();
    let max_width = ui.available_width();
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id, color, ..Default::default() },
    );
    job.wrap = TextWrapping {
        max_width,
        max_rows: if open { usize::MAX } else { collapsed_lines.max(1) },
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let elided = galley.elided;
    ui.add(Label::new(galley));
    if !elided && !open {
        return;
    }
    let toggle = if open {
        format!("{} less", icons::CHEV_OPEN)
    } else {
        format!("{} more", icons::CHEV_CLOSED)
    };
    if ui.small_button(toggle).clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(state_id, !open));
    }
}

/// First `n` chars of `s` plus an ellipsis, cut only on char boundaries.
pub fn truncate_chars(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((cut, _)) => format!("{}…", &s[..cut]),
        None => s.to_string(),
    }
}

/// True when `text` carries leaked tool-call markup.
pub fn markup_leak(text: &str) -> bool {
    MARKUP_MARKERS.iter().any(|m| text.contains(m))
}

/// MM/DD/YYYY in UTC; takes `&database::schema::Datetime` by deref.
pub fn fmt_date(dt: &DateTime<Utc>) -> String {
    dt.format(DATE_FMT).to_string()
}

/// MM/DD/YYYY HH:MM in UTC; takes `&database::schema::Datetime` by deref.
pub fn fmt_date_time(dt: &DateTime<Utc>) -> String {
    dt.format(DATE_TIME_FMT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const ARROWS: &str = "flash BIOS — then reseat → retest";

    #[test]
    fn truncation_cuts_on_char_boundaries() {
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
        assert_eq!(truncate_chars(ARROWS, 11), "flash BIOS …");
        assert_eq!(truncate_chars(ARROWS, 12), "flash BIOS —…");
    }

    #[test]
    fn truncation_never_splits_a_multibyte_char() {
        for n in 0..=ARROWS.chars().count() + 4 {
            let out = truncate_chars(ARROWS, n);
            assert!(out.chars().count() <= n + 1, "n={n} produced {out:?}");
        }
    }

    #[test]
    fn dates_use_the_house_format() {
        let dt = Utc.with_ymd_and_hms(2026, 7, 24, 21, 8, 37).unwrap();
        assert_eq!(fmt_date(&dt), "07/24/2026");
        assert_eq!(fmt_date_time(&dt), "07/24/2026 21:08");
    }

    #[test]
    fn a_surreal_datetime_reaches_fmt_date_by_deref() {
        let dt = Utc.with_ymd_and_hms(2026, 7, 24, 21, 8, 37).unwrap();
        let wrapped: database::schema::Datetime = dt.into();
        assert_eq!(fmt_date(&wrapped), "07/24/2026");
    }

    #[test]
    fn leaked_tool_markup_is_detected() {
        assert!(markup_leak("<parameter name=\"fix\">Order of operations: (1) Flash BIOS"));
        assert!(markup_leak("<parameter name=\"confidence\">medium"));
        assert!(!markup_leak("DPC watchdog — update rtwlane.sys → 6001.1.0.0"));
    }
}
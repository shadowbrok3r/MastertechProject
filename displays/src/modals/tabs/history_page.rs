//! Task History Page - displays the history of changes made to a task

use chrono::{DateTime, Utc};
use database::schema::TaskHistory;
use eframe::egui::{Grid, ScrollArea, Ui, Vec2};

use crate::ui_tools::{
    controls::{self, Metrics, Section, Tokens},
    icons::svg,
    theme,
};

/// Displays the task history page with collapsible entries showing diffs
pub fn display_history_page(ui: &mut Ui, history: &[TaskHistory], _avail_size: Vec2) {
    ui.vertical(|ui| {
        if history.is_empty() {
            controls::empty_state(
                ui,
                svg::CLOCK_COUNTER_CLOCKWISE,
                "No history yet",
                "No history records found for this task.",
                &[],
            );
            return;
        }

        let m = Metrics::get(ui.ctx());
        ScrollArea::vertical()
            .auto_shrink(false)
            .max_height(550.0)
            .show(ui, |ui| {
                for (idx, record) in history.iter().enumerate() {
                    render_history_entry(ui, record, idx);
                    ui.add_space(m.gap.y);
                }
            });
    });
}

/// Renders a single history entry as a collapsing header
fn render_history_entry(ui: &mut Ui, record: &TaskHistory, idx: usize) {
    let date_str = format_datetime(&record.created_at);
    let header_text = format!("{} - {}", record.username, date_str);

    Section::new(format!("history_{}", idx), &header_text)
        .icon(svg::USER_CIRCLE)
        .default_open(idx == 0)
        .show(ui, |ui| {
            render_diff_content(ui, &record.diff);
        });
}

/// Formats a Datetime for display
fn format_datetime(dt: &database::schema::Datetime) -> String {
    let date: DateTime<Utc> = dt.clone().into();
    date.date_naive().to_string()
}

/// Renders the diff content inside a history entry
fn render_diff_content(ui: &mut Ui, diff: &serde_json::Value) {
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    match diff {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                controls::icon_label(ui, svg::INFO, "No changes recorded.");
                return;
            }

            Grid::new(ui.next_auto_id())
                .num_columns(3)
                .spacing([2.0 * m.gap.x, m.gap.y])
                .striped(true)
                .min_col_width(150.)
                .max_col_width(200.)
                .show(ui, |ui| {
                    controls::icon_label(ui, svg::TAG, "Field");
                    controls::icon_label(ui, svg::MINUS_CIRCLE, "Old Value");
                    controls::icon_label(ui, svg::PLUS_CIRCLE, "New Value");
                    ui.end_row();

                    for (field_name, change) in map {
                        let old_val = change
                            .get("old")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let new_val = change
                            .get("new")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");

                        ui.colored_label(t.text_dim, format_field_name(field_name));

                        let old_display = if old_val.is_empty() { "(empty)" } else { old_val };
                        ui.colored_label(theme::error(ui), old_display);

                        let new_display = if new_val.is_empty() { "(empty)" } else { new_val };
                        ui.colored_label(theme::success(ui), new_display);

                        ui.end_row();
                    }
                });
        }
        serde_json::Value::Null => {
            controls::icon_label(ui, svg::INFO, "No diff data available.");
        }
        _ => {
            let text = format!("Unexpected diff format: {:?}", diff);
            controls::status_label(ui, svg::WARNING, &text, theme::warn(ui));
        }
    }
}

/// Converts snake_case field names to Title Case for display
fn format_field_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

//! Task History Page - displays the history of changes made to a task

use chrono::{DateTime, Utc};
use database::schema::TaskHistory;
use eframe::egui::{
    CollapsingHeader, Color32, Grid, RichText, ScrollArea, Ui, Vec2,
};

/// Displays the task history page with collapsible entries showing diffs
pub fn display_history_page(ui: &mut Ui, history: &[TaskHistory], _avail_size: Vec2) {
    ui.vertical(|ui| {
        ui.heading("Task History");
        ui.add_space(10.0);

        if history.is_empty() {
            ui.colored_label(
                Color32::from_rgb(150, 150, 150),
                "No history records found for this task.",
            );
            return;
        }

        ScrollArea::vertical()
            .auto_shrink(false)
            .max_height(550.0)
            .show(ui, |ui| {
                for (idx, record) in history.iter().enumerate() {
                    render_history_entry(ui, record, idx);
                    ui.add_space(5.0);
                }
            });
    });
}

/// Renders a single history entry as a collapsing header
fn render_history_entry(ui: &mut Ui, record: &TaskHistory, idx: usize) {
    // Format the date nicely
    let date_str = format_datetime(&record.created_at);
    let header_text = format!("{} - {}", record.username, date_str);

    CollapsingHeader::new(RichText::new(&header_text).strong().size(14.0))
        .id_salt(format!("history_{}", idx))
        .default_open(idx == 0) // Open the most recent entry by default
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
    match diff {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                ui.label("No changes recorded.");
                return;
            }

            Grid::new(ui.next_auto_id())
                .num_columns(3)
                .spacing([15.0, 6.0])
                .striped(true)
                .min_col_width(200.)
                .show(ui, |ui| {
                    // Header row
                    ui.label(RichText::new("Field").strong().color(Color32::from_rgb(150, 180, 220)));
                    ui.label(RichText::new("Old Value").strong().color(Color32::from_rgb(255, 150, 150)));
                    ui.label(RichText::new("New Value").strong().color(Color32::from_rgb(150, 255, 150)));
                    ui.end_row();

                    // Data rows
                    for (field_name, change) in map {
                        let old_val = change
                            .get("old")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let new_val = change
                            .get("new")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");

                        // Format field name nicely
                        let display_name = format_field_name(field_name);
                        
                        ui.label(RichText::new(&display_name).color(Color32::from_rgb(200, 200, 200)));
                        
                        // Old value in red-ish
                        let old_display = if old_val.is_empty() { "(empty)" } else { old_val };
                        ui.label(RichText::new(old_display).color(Color32::from_rgb(255, 180, 180)));
                        
                        // New value in green-ish
                        let new_display = if new_val.is_empty() { "(empty)" } else { new_val };
                        ui.label(RichText::new(new_display).color(Color32::from_rgb(180, 255, 180)));
                        
                        ui.end_row();
                    }
                });
        }
        serde_json::Value::Null => {
            ui.label("No diff data available.");
        }
        _ => {
            ui.label(format!("Unexpected diff format: {:?}", diff));
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

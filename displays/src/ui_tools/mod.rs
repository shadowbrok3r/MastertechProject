use eframe::egui::{text::LayoutJob, Button, Color32, FontId, Margin, RichText, Style, TextFormat, Ui, Widget};
use database::schema::LiveTaskPayload;
use std::collections::BTreeSet;
use anyhow::Context;
use regex::Regex;

use crate::TaskUiActions;

pub mod autocomplete;
pub mod carl_dark;
pub mod mention_handler;
pub mod toasts;
pub mod tokyo_dark;
pub mod rerun_mtech;
pub mod theme_chrome;
pub mod theme_config;
pub mod notification_center;
pub mod icons;
pub mod selection_stats;

pub use mtech_ui::{dock_style, egui_logger, theme};

const ZSTD_LEVEL: i32 = 9;

/// egui `Style` layout is not stable across egui versions, so persist it as JSON (field-named,
/// tolerant of added/removed fields) rather than positional bincode. zstd keeps it compact.
pub fn encode_style(message: &Style) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::to_vec(message).context("serialize style to JSON")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&json), ZSTD_LEVEL).context("zstd")?;
    Ok(compressed)
}

pub fn decode_style(packet: &[u8]) -> anyhow::Result<Style> {
    let json = zstd::decode_all(packet).context("zstd")?;
    style_from_json(&json)
}

/// Deserialize an egui `Style` from JSON, migrating across egui versions. On a strict-decode
/// failure (egui added/renamed a `Style`/`Visuals`/nested field — e.g. 0.35's
/// `color_transfer_function`, where one missing field in a non-`#[serde(default)]` struct fails
/// the whole parse), overlay the stored values onto the current `Style::default()`: new fields
/// take their default, removed/renamed fields are dropped, and matching values (colors, spacing,
/// text styles, …) are preserved. This keeps saved themes working across egui upgrades.
pub fn style_from_json(json: &[u8]) -> anyhow::Result<Style> {
    match serde_json::from_slice::<Style>(json) {
        Ok(style) => Ok(style),
        Err(e) => {
            log::warn!("theme strict-decode failed ({e}); migrating against egui Style defaults");
            let stored: serde_json::Value =
                serde_json::from_slice(json).context("theme is not valid JSON; cannot migrate")?;
            let default =
                serde_json::to_value(Style::default()).context("serialize default Style")?;
            match serde_json::from_value::<Style>(merge_over_default(default, stored)) {
                Ok(style) => {
                    log::info!("theme migrated to the current egui Style schema");
                    Ok(style)
                }
                Err(e2) => {
                    log::error!("theme migration failed ({e2}); using default Style");
                    Ok(Style::default())
                }
            }
        }
    }
}

/// Recursively overlay `stored` onto `default`, keyed by the default's structure: objects recurse
/// over the default's keys (so new fields are kept and removed ones dropped), scalars/arrays take
/// the stored value, and a shape mismatch keeps the default. Enum-tagged objects stay single-variant
/// (only the default's keys survive), so the result always re-parses into the current `Style`.
fn merge_over_default(default: serde_json::Value, stored: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (default, stored) {
        (Value::Object(mut d), Value::Object(s)) => {
            for (k, dv) in d.iter_mut() {
                if let Some(sv) = s.get(k) {
                    *dv = merge_over_default(dv.take(), sv.clone());
                }
            }
            Value::Object(d)
        }
        (Value::Object(d), _) => Value::Object(d),
        (_, stored) => stored,
    }
}

pub fn find_task_in_description(
    notification_description: &str,
    task_names: &BTreeSet<String>, // BTreeSet of task names
) -> Vec<String> {
    // Define multiple regex patterns for different task formats
    let regex_patterns = vec![
        Regex::new(r"in task (.+)").unwrap(), // Matches: "in task {task name}"
        Regex::new(r"(.+) assigned to you").unwrap(), // Matches: "{task name} assigned to you"
    ];

    // Iterate through each regex pattern and try to find matches
    let mut matches: Vec<String> = Vec::new();
    for task_name_regex in regex_patterns {
        // Use regex to find the task name in the description
        if let Some(caps) = task_name_regex.captures(notification_description) {
            if let Some(match_task_name) = caps.get(1) {
                // Get the first capture group (task name)
                let task_name = match_task_name.as_str().to_string();

                // Check if the extracted task name is in the set of task names
                if task_names.contains(&task_name) {
                    matches.push(task_name); // Add the matching task name to the result
                }
            }
        }
    }

    matches
}

pub fn show_notification(
    ui: &mut Ui,
    notification_description: &str,
    task_names: &BTreeSet<String>,
    ui_actions_tx: crossbeam::channel::Sender<TaskUiActions>,
    tasks: &Vec<LiveTaskPayload>,
) {
    // Find task names in the notification description using regex
    let matches = find_task_in_description(notification_description, task_names);

    // We assume only one match for simplicity; handle multiple matches if necessary
    if let Some(task_name) = matches.get(0) {
        // Find where the task name is in the notification description
        if let Some(pos) = notification_description.find(task_name) {
            // Split the text into before, task name, and after
            let before = &notification_description[..pos];
            let after = &notification_description[pos + task_name.len()..];

            // Display the text parts with different formatting
            eframe::egui::Frame::new()
                .fill(ui.style().visuals.window_fill)
                .corner_radius(eframe::egui::CornerRadius::same(12))
                .inner_margin(Margin::same(15))
                .outer_margin(Margin::same(5))
                .show(ui, |ui| {
                    // info!("{pos:?}, {before:?}, {task_name:?}, {after:?}");
                    ui.horizontal_wrapped(|ui| {
                        // Show the text before the task name
                        ui.label(RichText::new(before));

                        // Show the task name in a different color (e.g., blue)
                        let color = Color32::from_rgba_premultiplied(42, 222, 192, 60);
                        if Button::new(
                            RichText::new(task_name)
                                .color(color),
                        )
                        .ui(ui)
                        .clicked()
                        {
                            let task = tasks.iter().find(|&x| {
                                x.task_name == *task_name
                                    || format!("{}", x.service_number.clone().unwrap_or_default())
                                        == format!("{}", *task_name)
                            });

                            if let Some(task) = task {
                                let _ = ui_actions_tx
                                    .try_send(TaskUiActions::OpenTaskModal(task.clone()));
                            }
                        }

                        // Show the text after the task name
                        ui.label(after);
                    });
                });
        } else {
            // If no task name is found, display the whole description normally
            ui.label(notification_description);
        }
    } else {
        // If no task name is matched, just show the description
        ui.label(notification_description);
    }
}

/// Function to color text between two delimiters
pub fn color_between_delimiters(
    layout_job: &mut LayoutJob,
    text: &str,
    delimiters: (&str, &str),
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(delimiters.0) {
        let after_start = start_idx + delimiters.0.len();
        if let Some(end_idx) = remaining_text[after_start..].find(delimiters.1) {
            let end_idx = after_start + end_idx;

            // Append the text before the first delimiter
            layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

            // Append the first delimiter itself
            layout_job.append(delimiters.0, 0.0, TextFormat::default());

            // Append the text between the delimiters with the given color
            layout_job.append(
                &remaining_text[after_start..end_idx],
                0.0,
                TextFormat::simple(FontId::default(), color),
            );

            // Append the second delimiter
            layout_job.append(delimiters.1, 0.0, TextFormat::default());

            // Update remaining_text to the part after the second delimiter
            remaining_text = remaining_text[end_idx + delimiters.1.len()..].to_string();
        } else {
            break;
        }
    }

    remaining_text
}

pub fn highlight_text(
    text: &str,
    pattern: &str,
    delimiters: (&str, &str),
    pattern_color: Color32,
    delimiter_color: Color32,
    base_format: &TextFormat,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut i = 0;
    let text_len = text.len();

    while i < text_len {
        let remaining_text = &text[i..];

        // Check if remaining_text is empty before proceeding
        if remaining_text.is_empty() {
            break;
        }

        // Check if the text at position i matches the start delimiter
        if remaining_text.starts_with(delimiters.0) {
            // Append the start delimiter with base format
            let delimiter_len = delimiters.0.len();
            job.append(&text[i..i + delimiter_len], 0.0, base_format.clone());
            i += delimiter_len;

            // Ensure we don't go out of bounds
            if i >= text_len {
                break;
            }

            // Find the end delimiter
            let end_delimiter_pos = text[i..].find(delimiters.1);

            let end_idx = match end_delimiter_pos {
                Some(pos) => pos,
                None => text_len - i, // No end delimiter found, take the rest of the text
            };

            // Append the text between delimiters with delimiter color
            let mut format = base_format.clone();
            format.color = delimiter_color;

            // Ensure we don't go out of bounds
            if i + end_idx <= text_len {
                job.append(&text[i..i + end_idx], 0.0, format);
                i += end_idx;
            } else {
                // Not enough bytes left, break the loop
                break;
            }

            // Check if end delimiter is present
            if i + delimiters.1.len() <= text_len && text[i..].starts_with(delimiters.1) {
                // Append the end delimiter with base format
                job.append(&text[i..i + delimiters.1.len()], 0.0, base_format.clone());
                i += delimiters.1.len();
            }
        }
        // Check if the text at position i matches the pattern
        else if remaining_text.starts_with(pattern) {
            // Append the pattern with the pattern color
            let pattern_len = pattern.len();
            if i + pattern_len <= text_len {
                let mut format = base_format.clone();
                format.color = pattern_color;
                job.append(&text[i..i + pattern_len], 0.0, format);
                i += pattern_len;
            } else {
                // Not enough bytes left, break the loop
                break;
            }
        } else {
            // Append the current character with base format
            if let Some(c) = remaining_text.chars().next() {
                let c_len = c.len_utf8();
                if i + c_len <= text_len {
                    job.append(&text[i..i + c_len], 0.0, base_format.clone());
                    i += c_len;
                } else {
                    // Not enough bytes left, break the loop
                    break;
                }
            } else {
                // No more characters, break the loop
                break;
            }
        }
    }

    job
}

/// Function to color text that matches a specific substring
pub fn color_matching_text(
    layout_job: &mut LayoutJob,
    text: &str,
    pattern: &str,
    color: Color32,
) -> String {
    let mut remaining_text = String::from(text);

    while let Some(start_idx) = remaining_text.find(pattern) {
        let end_idx = start_idx + pattern.len();

        // Append the text before the pattern
        layout_job.append(&remaining_text[..start_idx], 0.0, TextFormat::default());

        // Append the pattern with the given color
        layout_job.append(pattern, 0.0, TextFormat::simple(FontId::default(), color));

        // Update remaining_text to the part after the pattern
        remaining_text = remaining_text[end_idx..].to_string();
    }

    remaining_text
}

use eframe::egui::{text::LayoutJob, Color32, FontFamily, FontId, Style, TextFormat, Ui};
use database::schema::LiveTaskPayload;
use std::collections::BTreeSet;
use anyhow::Context;
use regex::Regex;

use crate::TaskUiActions;

pub mod autocomplete;
pub mod carl_dark;
pub mod font_atlas_watch;
pub mod mention_handler;
pub mod toasts;
pub mod tokyo_dark;
pub mod rerun_mtech;
pub mod mtech_glass;
pub mod mtech_studio;
pub mod neon_glass;
pub mod galactic_glass;
pub mod soft_glass;
pub mod glass_backdrop;
pub mod glass_card;
pub mod agent_chat;
pub mod chat_bubble;
pub mod framed_controls;
pub mod theme_chrome;
pub mod theme_config;
pub mod terminal_font;
// ratatui is a non-wasm dependency.
#[cfg(not(target_arch = "wasm32"))]
pub mod tui_theme;
pub mod notification_center;
pub mod command_bar;
pub mod do_not_disturb;
pub mod dump_text;
pub mod hex_json;
pub mod icons;
pub mod info_card;
pub mod list_row;
pub mod plots;
pub mod selection_stats;
#[cfg(test)]
pub(crate) mod headless_render;
#[cfg(test)]
mod kit_tests;
#[cfg(test)]
mod ratchet_tests;
#[cfg(test)]
mod title_bar_tests;

pub use mtech_ui::{
    controls, dock_style, egui_logger, fonts, metrics, theme, tokens, widget_theme,
};

const ZSTD_LEVEL: i32 = 9;

const STYLE_KEY: &str = "style";
const GLASS_KEY: &str = "glass";
const DENSITY_KEY: &str = "density";
const TEXT_SCALE_KEY: &str = "text_scale";

/// A saved theme: the egui `Style` plus the parts of a theme that have no `Style` slot.
///
/// The account record (`user_settings.color_scheme`) holds one of these, so a theme follows the
/// operator between machines whole — including its backdrop-blur material, which egui has nowhere
/// to put.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SavedTheme {
    pub style: Style,
    /// `None` on a record written before the glass material existed, or on an imported bare
    /// `egui::Style` file. Callers recover it from the style's matching preset instead of guessing.
    #[serde(default)]
    pub glass: Option<crate::ui_tools::glass_backdrop::GlassParams>,
    #[serde(default)]
    pub density: metrics::Density,
    #[serde(default)]
    pub text_scale: fonts::TextScale,
}

impl SavedTheme {
    /// The theme as currently applied to `ctx` — the live style plus the glass material sitting on
    /// the context. This is what Save persists.
    pub fn current(ctx: &eframe::egui::Context) -> Self {
        let mut style = (*ctx.global_style()).clone();
        strip_weight_families(&mut style);
        Self {
            style,
            glass: Some(glass_backdrop::params(ctx)),
            density: metrics::density(ctx),
            text_scale: fonts::text_scale(ctx),
        }
    }
}

/// Resets text styles in the overlay's weight families to `Proportional`.
fn strip_weight_families(style: &mut Style) {
    for font in style.text_styles.values_mut() {
        if let FontFamily::Name(name) = &font.family
            && (&**name == fonts::MEDIUM || &**name == fonts::SEMIBOLD)
        {
            font.family = FontFamily::Proportional;
        }
    }
}

/// egui `Style` layout is not stable across egui versions, so persist a theme as JSON
/// (field-named, tolerant of added/removed fields) rather than positional bincode. zstd keeps it
/// compact.
pub fn encode_theme(theme: &SavedTheme) -> anyhow::Result<Vec<u8>> {
    let json = theme_to_json(theme)?;
    zstd::encode_all(std::io::Cursor::new(&json), ZSTD_LEVEL).context("zstd")
}

pub fn decode_theme(packet: &[u8]) -> anyhow::Result<SavedTheme> {
    let json = zstd::decode_all(packet).context("zstd")?;
    theme_from_json(&json)
}

/// Serialize a theme to the on-disk/on-record JSON shape, for exporting to a file.
pub fn theme_to_json(theme: &SavedTheme) -> anyhow::Result<Vec<u8>> {
    let mut theme = theme.clone();
    strip_weight_families(&mut theme.style);
    serde_json::to_vec(&theme).context("serialize theme to JSON")
}

/// Read a theme from JSON in either shape: the wrapper this crate writes, or a bare `egui::Style`
/// object — which is what every record written before the glass material, and every scheme exported
/// or hand-authored against egui's own `Style` schema, looks like. `egui::Style` has no `style`
/// field, so the presence of that key is what tells the two apart.
pub fn theme_from_json(json: &[u8]) -> anyhow::Result<SavedTheme> {
    let value: serde_json::Value =
        serde_json::from_slice(json).context("theme is not valid JSON")?;
    match value {
        serde_json::Value::Object(mut map) if map.contains_key(STYLE_KEY) => {
            let glass = take_field(&mut map, GLASS_KEY);
            let density = take_field(&mut map, DENSITY_KEY).unwrap_or_default();
            let text_scale = take_field(&mut map, TEXT_SCALE_KEY).unwrap_or_default();
            let style = map.remove(STYLE_KEY).unwrap_or(serde_json::Value::Null);
            Ok(SavedTheme { style: style_from_value(style), glass, density, text_scale })
        }
        bare => Ok(SavedTheme {
            style: style_from_value(bare),
            glass: None,
            density: Default::default(),
            text_scale: Default::default(),
        }),
    }
}

/// Removes `key` from `map` and decodes it, or `None` when absent or malformed.
fn take_field<T: serde::de::DeserializeOwned>(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<T> {
    map.remove(key).and_then(|v| serde_json::from_value(v).ok())
}

/// Deserialize an egui `Style` from JSON, migrating across egui versions.
pub fn style_from_json(json: &[u8]) -> anyhow::Result<Style> {
    let value: serde_json::Value =
        serde_json::from_slice(json).context("theme is not valid JSON; cannot migrate")?;
    Ok(style_from_value(value))
}

/// On a strict-decode failure (egui added/renamed a `Style`/`Visuals`/nested field — e.g. 0.35's
/// `color_transfer_function`, where one missing field in a non-`#[serde(default)]` struct fails the
/// whole parse), overlay the stored values onto the current `Style::default()`: new fields take
/// their default, removed/renamed fields are dropped, and matching values (colors, spacing, text
/// styles, …) are preserved. This keeps saved themes working across egui upgrades, and folds an
/// unsalvageable style to the default rather than failing a login.
fn style_from_value(value: serde_json::Value) -> Style {
    match serde_json::from_value::<Style>(value.clone()) {
        Ok(style) => style,
        Err(e) => {
            log::debug!(
                "stored theme predates the current egui Style schema ({e}); migrating against egui Style defaults"
            );
            let default = serde_json::to_value(Style::default()).unwrap_or_default();
            match serde_json::from_value::<Style>(merge_over_default(default, value)) {
                Ok(style) => {
                    log::debug!("theme migrated to the current egui Style schema");
                    style
                }
                Err(e2) => {
                    log::error!("theme migration failed ({e2}); using default Style");
                    Style::default()
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
    if let Some(task_name) = matches.first() {
        // Find where the task name is in the notification description
        if let Some(pos) = notification_description.find(task_name) {
            let before = &notification_description[..pos];
            let after = &notification_description[pos + task_name.len()..];
            ui.horizontal_wrapped(|ui| {
                ui.label(before);
                let t = tokens::Tokens::get(ui.ctx());
                let link = controls::IconLink::new(task_name)
                    .icon(icons::svg::CLIPBOARD_TEXT)
                    .tone(t.info)
                    .show(ui);
                if link.clicked() {
                    let task = tasks.iter().find(|x| {
                        x.task_name == *task_name
                            || x.service_number.as_deref().unwrap_or_default() == task_name.as_str()
                    });
                    if let Some(task) = task {
                        let _ = ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task.clone()));
                    }
                }
                ui.label(after);
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

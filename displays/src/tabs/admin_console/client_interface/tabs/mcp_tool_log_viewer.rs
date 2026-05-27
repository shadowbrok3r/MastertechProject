use crate::mcp_tool_log::{self, McpToolCallLog, McpToolCallStatus};
use crate::ui_tools::icons::{self, icon_sized};
use crate::ui_tools::theme;
use eframe::egui::{Color32, RichText, ScrollArea, TextEdit, Ui};
use std::collections::HashSet;

pub struct McpToolLogViewer {
    expanded: HashSet<String>,
    filter: String,
    show_completed: bool,
    auto_scroll: bool,
}

impl Default for McpToolLogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpToolLogViewer {
    pub fn new() -> Self {
        Self {
            expanded: HashSet::new(),
            filter: String::new(),
            show_completed: true,
            auto_scroll: true,
        }
    }

    pub fn display(&mut self, ui: &mut Ui, connection_string: &str) {
        // Merges per-client entries with the global feed so local MCP calls
        // (query_surrealdb, list_plugins, etc.) show up alongside the
        // tools that targeted this specific client.
        let entries = mcp_tool_log::get_for_client(connection_string);
        let pending = entries
            .iter()
            .filter(|e| matches!(e.status, McpToolCallStatus::Pending))
            .count();

        ui.horizontal(|ui| {
            ui.label(RichText::new("MCP Tool Calls").strong());
            ui.separator();
            ui.label(format!("{} total", entries.len()));
            if pending > 0 {
                ui.colored_label(
                    Color32::from_rgb(255, 200, 80),
                    format!("• {pending} in flight"),
                );
            }
            ui.separator();
            ui.checkbox(&mut self.show_completed, "Show completed");
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            ui.separator();
            ui.label("Filter:");
            ui.add(TextEdit::singleline(&mut self.filter).desired_width(160.0));
            if ui.button("Clear completed").clicked() {
                mcp_tool_log::clear(connection_string);
            }
        });
        ui.separator();

        let filter = self.filter.to_ascii_lowercase();
        let filter_active = !filter.is_empty();
        let filtered: Vec<McpToolCallLog> = entries
            .into_iter()
            .filter(|e| {
                if !self.show_completed && !matches!(e.status, McpToolCallStatus::Pending) {
                    return false;
                }
                if filter_active {
                    let hay = format!("{} {} {}", e.plugin_id, e.tool_name, e.args_json)
                        .to_ascii_lowercase();
                    if !hay.contains(&filter) {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Copy button: dumps the currently-visible entries (so filter +
        // "Show completed" act as a way to scope what gets copied). Placed
        // here so the button is responsive even when the list is empty.
        ui.horizontal(|ui| {
            let label = format!("Copy {} entry/entries", filtered.len());
            let copy_enabled = !filtered.is_empty();
            if ui
                .add_enabled(copy_enabled, eframe::egui::Button::new(label))
                .on_hover_text(
                    "Copy the currently-visible MCP tool calls (respects the filter and \"Show completed\" toggle) to the clipboard as plain text.",
                )
                .clicked()
            {
                let dump = format_entries_for_clipboard(&filtered);
                ui.ctx().copy_text(dump);
            }
        });
        ui.add_space(2.0);

        if filtered.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No MCP tool calls to show.")
                        .color(Color32::GRAY)
                        .small(),
                );
                ui.label(
                    RichText::new(
                        "Calls proxied through this client's Web Console session will appear here.",
                    )
                    .color(Color32::from_rgb(120, 120, 140))
                    .small(),
                );
            });
            return;
        }

        let mut scroll = ScrollArea::vertical().auto_shrink([false, false]);
        if self.auto_scroll {
            scroll = scroll.stick_to_bottom(true);
        }
        scroll.show(ui, |ui| {
            for entry in &filtered {
                self.row(ui, entry);
            }
        });
    }

    fn row(&mut self, ui: &mut Ui, entry: &McpToolCallLog) {
        let is_expanded = self.expanded.contains(&entry.request_id);
        let (glyph, color) = status_glyph(ui, &entry.status);

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(icon_sized(glyph, 13.0).color(color));
                let header = format!(
                    "{}::{}",
                    short_id(&entry.plugin_id),
                    entry.tool_name
                );
                let toggle = ui.selectable_label(is_expanded, RichText::new(header).monospace());
                if toggle.clicked() {
                    if is_expanded {
                        self.expanded.remove(&entry.request_id);
                    } else {
                        self.expanded.insert(entry.request_id.clone());
                    }
                }
                ui.with_layout(
                    eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                    |ui| {
                        let elapsed = format_elapsed(entry.elapsed_ms());
                        ui.label(
                            RichText::new(elapsed)
                                .color(Color32::from_rgb(160, 160, 170))
                                .small(),
                        );
                        ui.label(
                            RichText::new(format!("req={}", short_id(&entry.request_id)))
                                .color(Color32::from_rgb(120, 120, 140))
                                .small()
                                .monospace(),
                        );
                    },
                );
            });

            if !is_expanded {
                let preview = arg_preview(&entry.args_json);
                if !preview.is_empty() {
                    ui.label(
                        RichText::new(preview)
                            .color(Color32::from_rgb(180, 180, 200))
                            .small()
                            .monospace(),
                    );
                }
                return;
            }

            ui.separator();
            ui.label(RichText::new("Arguments").color(Color32::from_rgb(180, 200, 230)).small());
            json_block(ui, &entry.args_json, &format!("args-{}", entry.request_id));

            ui.add_space(4.0);
            ui.label(RichText::new("Result").color(Color32::from_rgb(180, 200, 230)).small());
            match (&entry.status, entry.result_json.as_deref()) {
                (McpToolCallStatus::Pending, _) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.colored_label(
                            Color32::from_rgb(255, 200, 80),
                            "Awaiting response from remote client…",
                        );
                    });
                }
                (_, Some(body)) => json_block(ui, body, &format!("res-{}", entry.request_id)),
                (_, None) => {
                    ui.colored_label(Color32::GRAY, "(no result body)");
                }
            }
        });
    }
}

fn status_glyph(ui: &Ui, status: &McpToolCallStatus) -> (&'static str, Color32) {
    match status {
        McpToolCallStatus::Pending => (icons::STATUS_WAIT, theme::warn(ui)),
        McpToolCallStatus::Success => (icons::STATUS_READY, theme::info(ui)),
        McpToolCallStatus::Error => (icons::STATUS_ERR, ui.style().visuals.error_fg_color),
    }
}

fn short_id(s: &str) -> String {
    const MAX: usize = 24;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…", &s[..MAX])
    }
}

fn format_elapsed(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let secs = ms / 1000;
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

fn arg_preview(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "{}" || trimmed == "null" {
        return String::new();
    }
    const MAX: usize = 140;
    let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX {
        collapsed
    } else {
        let cut: String = collapsed.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

/// Plain-text dump of the supplied entries for the clipboard. Mirrors the
/// expanded-row layout so what the user copies looks like what they see,
/// minus the egui styling.
fn format_entries_for_clipboard(entries: &[McpToolCallLog]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== MCP Tool Calls — {} entry/entries ===\n\n",
        entries.len()
    ));
    for (i, e) in entries.iter().enumerate() {
        let status = match e.status {
            McpToolCallStatus::Pending => "PENDING",
            McpToolCallStatus::Success => "OK",
            McpToolCallStatus::Error => "ERR",
        };
        out.push_str(&format!(
            "[{}/{}] {} {}::{}  req={}  elapsed={}\n",
            i + 1,
            entries.len(),
            status,
            e.plugin_id,
            e.tool_name,
            e.request_id,
            format_elapsed(e.elapsed_ms()),
        ));
        out.push_str("Arguments:\n");
        out.push_str(&pretty_json_or_raw(&e.args_json));
        out.push_str("\n\nResult:\n");
        match (&e.status, e.result_json.as_deref()) {
            (McpToolCallStatus::Pending, _) => out.push_str("(still pending)"),
            (_, Some(body)) => out.push_str(&pretty_json_or_raw(body)),
            (_, None) => out.push_str("(no result body)"),
        }
        out.push_str("\n\n");
    }
    out
}

fn pretty_json_or_raw(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| raw.to_string())
}

fn json_block(ui: &mut Ui, raw: &str, id_salt: &str) {
    let mut buf = pretty_json_or_raw(raw);
    ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(240.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add(
                TextEdit::multiline(&mut buf)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(4),
            );
        });
}

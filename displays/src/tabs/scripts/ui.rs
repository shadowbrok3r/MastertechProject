//! UI rendering for the scripts tab

use super::ScriptsTab;
use crate::plugins::push_widget_anchor;
use crate::scripts::{
    category_display_name, category_icon, LogLevel, ScriptCategory,
    ScriptLogEntry, ScriptStatus, CATEGORY_ORDER,
};
use eframe::egui::{self, Frame, RichText, ScrollArea, Stroke, Ui};

/// Stable slug for a `ScriptCategory` used in `remote_egui` anchor keys.
/// Matches the convention in `mcp_bridge.rs` INSTRUCTIONS: lowercase,
/// non-alphanumeric → '_'. Free-text variants get a slugified body.
fn category_anchor_slug(category: &ScriptCategory) -> String {
    match category {
        ScriptCategory::Tuneup => "tuneup".to_string(),
        ScriptCategory::Informational => "informational".to_string(),
        ScriptCategory::JunkwareRemoval => "junkware".to_string(),
        ScriptCategory::UserScripts(name) => format!("user.{}", slugify(name)),
        ScriptCategory::Custom(name) => format!("custom.{}", slugify(name)),
    }
}

/// Lowercase + non-alphanumeric → '_', collapse repeats, trim '_'.
fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_underscore = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Colors for the scripts UI
mod colors {
    use eframe::egui::Color32;

    pub const CATEGORY_HEADER: Color32 = Color32::from_rgb(138, 180, 248);
    pub const SELECTED: Color32 = Color32::from_rgb(46, 160, 126);
    pub const PENDING: Color32 = Color32::from_rgb(166, 172, 205);
    pub const RUNNING: Color32 = Color32::from_rgb(249, 226, 175);
    pub const COMPLETED: Color32 = Color32::from_rgb(166, 227, 161);
    pub const FAILED: Color32 = Color32::from_rgb(243, 139, 168);
    pub const SKIPPED: Color32 = Color32::from_rgb(147, 153, 178);
    
    pub const LOG_INFO: Color32 = Color32::from_rgb(205, 214, 244);
    pub const LOG_SUCCESS: Color32 = Color32::from_rgb(166, 227, 161);
    pub const LOG_WARNING: Color32 = Color32::from_rgb(249, 226, 175);
    pub const LOG_ERROR: Color32 = Color32::from_rgb(243, 139, 168);

    pub const PANEL_BG: Color32 = Color32::from_rgb(17, 17, 27);
    pub const QUEUE_ITEM_BG: Color32 = Color32::from_rgb(30, 30, 46);
}

impl ScriptsTab {
    /// Main render function for the scripts tab
    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive();

        // Top bar with service number and controls
        self.render_top_bar(ui);
        
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Main content area with three columns
        let available_width = ui.available_width();
        let panel_spacing = 8.0;
        let left_width = available_width * 0.25;
        let middle_width = available_width * 0.35;
        let right_width = available_width * 0.40 - panel_spacing * 2.0;

        ui.horizontal(|ui| {
            // Left panel - Categories
            ui.vertical(|ui| {
                ui.set_width(left_width);
                self.render_categories_panel(ui);
            });

            ui.add_space(panel_spacing);

            // Middle panel - Queue
            ui.vertical(|ui| {
                ui.set_width(middle_width);
                self.render_queue_panel(ui);
            });

            ui.add_space(panel_spacing);

            // Right panel - Logs
            ui.vertical(|ui| {
                ui.set_width(right_width);
                self.render_logs_panel(ui);
            });
        });
    }

    /// Render the top control bar
    fn render_top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Service #:");
            let service_field = ui.add(
                egui::TextEdit::singleline(&mut self.service_number_input)
                    .desired_width(120.0)
                    .hint_text("Enter SO#"),
            );
            push_widget_anchor("scripts.service_number", service_field.rect);

            ui.add_space(16.0);

            let add_btn = ui
                .button(RichText::new("➕ Add Selected to Queue").color(colors::SELECTED));
            push_widget_anchor("scripts.queue_add_btn", add_btn.rect);
            if add_btn.clicked() {
                self.queue_selected_scripts();
            }

            if self.state.queue.is_running() {
                let stop_btn = ui
                    .button(RichText::new("⏹ Stop").color(colors::FAILED));
                push_widget_anchor("scripts.stop_btn", stop_btn.rect);
                if stop_btn.clicked() {
                    self.stop_queue();
                }
            } else {
                let run_btn = ui
                    .button(RichText::new("▶ Run Queue").color(colors::COMPLETED));
                push_widget_anchor("scripts.run_btn", run_btn.rect);
                if run_btn.clicked() {
                    self.run_queue();
                }
            }

            let clear_btn = ui
                .button(RichText::new("🗑 Clear Queue").color(colors::PENDING));
            push_widget_anchor("scripts.queue_clear_btn", clear_btn.rect);
            if clear_btn.clicked() {
                self.clear_queue();
            }

            // Progress indicator
            if let Some((current, total)) = self.state.current_progress {
                ui.add_space(16.0);
                let progress = current as f32 / total as f32;
                ui.add(
                    egui::ProgressBar::new(progress)
                        .desired_width(150.0)
                        .text(format!(
                            "{}: {:.0}%",
                            self.state.current_script_name.as_deref().unwrap_or("..."),
                            progress * 100.0
                        )),
                );
            }

            // Queue status
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (completed, total) = self.state.queue.progress();
                if total > 0 {
                    ui.label(format!("Queue: {}/{}", completed, total));
                }
            });
        });
    }

    /// Render the categories panel (left)
    fn render_categories_panel(&mut self, ui: &mut Ui) {
        Frame::new()
            .fill(colors::PANEL_BG)
            .inner_margin(8.0)
            .outer_margin(0.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.heading(RichText::new("📚 Script Categories").color(colors::CATEGORY_HEADER));
                ui.add_space(8.0);

                ScrollArea::vertical()
                    .id_salt("categories_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for category in CATEGORY_ORDER.iter() {
                            self.render_category(ui, category);
                            ui.add_space(8.0);
                        }
                    });
            });
    }

    /// Render a single category with its scripts
    fn render_category(&mut self, ui: &mut Ui, category: &ScriptCategory) {
        let icon = category_icon(category);
        let name = category_display_name(category);
        let expanded = self.state.category_expanded.get(category).copied().unwrap_or(true);
        let cat_slug = category_anchor_slug(category);

        // Category header with collapse toggle
        ui.horizontal(|ui| {
            let collapse_icon = if expanded { "▼" } else { "▶" };
            let collapse_btn = ui.small_button(collapse_icon);
            push_widget_anchor(format!("scripts.{cat_slug}.collapse"), collapse_btn.rect);
            if collapse_btn.clicked() {
                self.state.category_expanded.insert(category.clone(), !expanded);
            }

            ui.label(RichText::new(format!("{} {}", icon, name)).strong().color(colors::CATEGORY_HEADER));

            // Select/deselect all button
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(scripts) = self.state.categories.get(category) {
                    let any_selected = scripts.iter().any(|s| s.is_selected());
                    let btn_text = if any_selected { "✗" } else { "✓" };
                    let btn_color = if any_selected { colors::FAILED } else { colors::COMPLETED };
                    let toggle_btn = ui.small_button(RichText::new(btn_text).color(btn_color));
                    push_widget_anchor(format!("scripts.{cat_slug}.toggle_all"), toggle_btn.rect);
                    if toggle_btn.clicked() {
                        if any_selected {
                            self.state.deselect_category(category);
                        } else {
                            self.state.select_category(category);
                        }
                    }
                }
            });
        });

        if expanded {
            if let Some(scripts) = self.state.categories.get_mut(category) {
                ui.indent(format!("category_{:?}", category), |ui| {
                    for script in scripts.iter_mut() {
                        let mut selected = script.is_selected();
                        let text_color = if selected { colors::SELECTED } else { colors::PENDING };

                        let item_slug = slugify(&script.name);
                        let cb = ui.checkbox(&mut selected, RichText::new(&script.name).color(text_color));
                        push_widget_anchor(
                            format!("scripts.{cat_slug}.{item_slug}"),
                            cb.rect,
                        );
                        if cb.changed() {
                            script.toggle_selection();
                        }
                    }
                });
            }
        }
    }

    /// Render the queue panel (middle)
    fn render_queue_panel(&mut self, ui: &mut Ui) {
        Frame::new()
            .fill(colors::PANEL_BG)
            .inner_margin(8.0)
            .outer_margin(0.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("📋 Script Queue").color(colors::CATEGORY_HEADER));
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("({} scripts)", self.state.queue.len())).small());
                });
                ui.add_space(8.0);

                if self.state.queue.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("Queue is empty").color(colors::PENDING).italics());
                        ui.label(RichText::new("Select scripts and click 'Add to Queue'").color(colors::PENDING).small());
                        ui.add_space(40.0);
                    });
                } else {
                    ScrollArea::vertical()
                        .id_salt("queue_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.render_queue_items(ui);
                        });
                }
            });
    }

    /// Render queue items with move up/down buttons
    fn render_queue_items(&mut self, ui: &mut Ui) {
        let queue_len = self.state.queue.len();
        let mut move_action: Option<(usize, usize)> = None;
        let mut remove_index: Option<usize> = None;

        for i in 0..queue_len {
            if let Some(item) = self.state.queue.items().get(i) {
                let border_color = match item.script.status {
                    ScriptStatus::Running => colors::RUNNING,
                    ScriptStatus::Completed => colors::COMPLETED,
                    ScriptStatus::Failed => colors::FAILED,
                    ScriptStatus::Selected => colors::SELECTED,
                    _ => colors::PENDING,
                };

                Frame::new()
                    .fill(colors::QUEUE_ITEM_BG)
                    .stroke(Stroke::new(1.0_f32, border_color))
                    .inner_margin(8.0)
                    .outer_margin(2.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Move up/down buttons
                            ui.vertical(|ui| {
                                if i > 0 {
                                    if ui.small_button("▲").clicked() {
                                        move_action = Some((i, i - 1));
                                    }
                                } else {
                                    ui.add_enabled(false, egui::Button::new("▲").small());
                                }
                                if i < queue_len - 1 {
                                    if ui.small_button("▼").clicked() {
                                        move_action = Some((i, i + 1));
                                    }
                                } else {
                                    ui.add_enabled(false, egui::Button::new("▼").small());
                                }
                            });

                            // Order number
                            ui.label(
                                RichText::new(format!("#{}", item.order + 1))
                                    .color(colors::CATEGORY_HEADER)
                                    .strong(),
                            );

                            ui.add_space(8.0);

                            // Script name and category
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&item.script.name).color(border_color));
                                ui.label(
                                    RichText::new(format!("{}", item.script.category))
                                        .color(colors::PENDING)
                                        .small(),
                                );
                            });

                            // Status and remove button
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Remove button
                                if ui.small_button("✕").clicked() {
                                    remove_index = Some(i);
                                }

                                let status_text = match item.script.status {
                                    ScriptStatus::Running => "⏳",
                                    ScriptStatus::Completed => "✓",
                                    ScriptStatus::Failed => "✗",
                                    ScriptStatus::Skipped => "⏭",
                                    _ => "",
                                };
                                if !status_text.is_empty() {
                                    ui.label(RichText::new(status_text).color(border_color).size(16.0));
                                }
                            });
                        });
                    });
            }
        }

        // Apply move action after iteration
        if let Some((from, to)) = move_action {
            self.state.queue.move_item(from, to);
        }

        // Apply remove action after iteration
        if let Some(idx) = remove_index {
            if let Some(item) = self.state.queue.items().get(idx) {
                let id = item.script.id.clone();
                self.state.queue.remove(&id);
            }
        }
    }

    /// Render the logs panel (right)
    fn render_logs_panel(&mut self, ui: &mut Ui) {
        Frame::new()
            .fill(colors::PANEL_BG)
            .inner_margin(8.0)
            .outer_margin(0.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("📜 Execution Log").color(colors::CATEGORY_HEADER));
                    ui.add_space(8.0);
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear").clicked() {
                            self.state.clear_logs();
                        }
                        ui.checkbox(&mut self.auto_scroll_logs, "Auto-scroll");
                    });
                });
                ui.add_space(8.0);

                if self.state.logs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("No log entries yet").color(colors::PENDING).italics());
                        ui.add_space(40.0);
                    });
                } else {
                    let scroll = ScrollArea::vertical()
                        .id_salt("logs_scroll")
                        .auto_shrink([false, false])
                        .stick_to_bottom(self.auto_scroll_logs);

                    scroll.show(ui, |ui| {
                        for entry in self.state.logs.iter() {
                            self.render_log_entry(ui, entry);
                        }
                    });
                }
            });
    }

    /// Render a single log entry
    fn render_log_entry(&self, ui: &mut Ui, entry: &ScriptLogEntry) {
        let color = match entry.level {
            LogLevel::Info => colors::LOG_INFO,
            LogLevel::Success => colors::LOG_SUCCESS,
            LogLevel::Warning => colors::LOG_WARNING,
            LogLevel::Error => colors::LOG_ERROR,
        };

        let icon = match entry.level {
            LogLevel::Info => "ℹ",
            LogLevel::Success => "✓",
            LogLevel::Warning => "⚠",
            LogLevel::Error => "✗",
        };

        ui.horizontal_wrapped(|ui| {
            // Timestamp
            let time_str = entry.timestamp.format("%H:%M:%S").to_string();
            ui.label(RichText::new(time_str).color(colors::PENDING).small().monospace());
            
            // Icon
            ui.label(RichText::new(icon).color(color));
            
            // Category and script
            ui.label(
                RichText::new(format!("[{}]", entry.script_name))
                    .color(colors::CATEGORY_HEADER)
                    .small(),
            );
            
            // Message
            ui.label(RichText::new(&entry.message).color(color));
        });
    }
}

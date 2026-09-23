//! The Scripts tab: catalog on the left, queue and details in the middle, run log on the right.

use displays::scripts::catalog::{CATALOG, Elevation, RebootHint, Requirement, ScriptDef};
use displays::scripts::executor::{ScriptOutcome, ScriptResult};
use displays::scripts::{
    CATEGORY_ORDER, LogLevel, ScriptCategory, ScriptItem, ScriptLogEntry, ScriptStatus,
    category_display_name,
};
use displays::ui_tools::framed_controls::{FramedSelectable, selectable_card};
use displays::ui_tools::info_card::{badge, expandable_text, kv_row};
use displays::ui_tools::{glass_card, icons, theme};
use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, Align, Button, Color32, FontId, Id, Label, Layout, ProgressBar, RichText, ScrollArea,
    TextEdit, TextFormat, TextStyle, Ui, vec2,
};
use egui_extras::{Size, StripBuilder};

use super::EguiScriptsTab;
use crate::app_state::MastertechContext;

/// Below this width the log stacks under the queue instead of taking its own column.
const THREE_COLUMN_MIN_WIDTH: f32 = 1000.0;
const CATALOG_WIDTH: f32 = 280.0;
const HEADER_HEIGHT: f32 = 34.0;
const STANDARD_PRESET: &str = "standard-tuneup";

/// What the details card is showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptFocus {
    Catalog(String),
    Queued(u64),
}

enum QueueAction {
    Move(usize, usize),
    Remove(u64),
    Focus(u64),
}

enum TransferAction {
    Start(Vec<String>, String),
    Cancel,
}

struct QueueRow {
    token: u64,
    order: usize,
    name: String,
    status: ScriptStatus,
}

impl MastertechContext {
    pub fn scripts(&mut self, ui: &mut Ui) {
        self.seed_script_context();
        self.reboot_modal(ui);
        self.data_transfer_modal(ui);

        StripBuilder::new(ui)
            .size(Size::exact(HEADER_HEIGHT))
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| scripts_header(ui, &mut self.scripts_tab));
                strip.cell(|ui| scripts_body(ui, &mut self.scripts_tab));
            });
    }

    /// Copies the ticket's service number in only when the ticket changes, so the field stays editable.
    fn seed_script_context(&mut self) {
        let ticket = &self.ticket_data.service_number;
        let tab = &mut self.scripts_tab;
        if !ticket.is_empty() && *ticket != tab.seeded_service_number {
            tab.service_number_input = ticket.clone();
            tab.seeded_service_number = ticket.clone();
        }
        if !self.customer_data.email.is_empty() {
            tab.customer_email = Some(self.customer_data.email.clone());
        }
    }

    fn reboot_modal(&mut self, ui: &mut Ui) {
        if !self.scripts_tab.reboot_prompt_open {
            return;
        }
        let modal = egui::Modal::new(Id::new("scripts_reboot_prompt")).show(ui.ctx(), |ui| {
            ui.set_max_width(380.0);
            ui.label(
                RichText::new(format!("{} Reboot required", icons::REFRESH))
                    .strong()
                    .color(theme::strong_text(ui)),
            );
            ui.add_space(4.0);
            ui.label("Webroot was re-keyed over an existing install.");
            ui.label(
                "A reboot finalizes the new device identity. MasterTech will relaunch after login.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let reboot = ui
                    .button(RichText::new("Reboot now").color(theme::success(ui)))
                    .clicked();
                let later = ui.button("Later").clicked();
                (reboot, later)
            })
            .inner
        });

        let (reboot, later) = modal.inner;
        if reboot {
            #[cfg(target_os = "windows")]
            crate::utilities::windows::reboot::spawn_reboot_with_relaunch(
                false,
                "Mastertech reboot to finalize Webroot activation",
            );
            self.scripts_tab.reboot_prompt_open = false;
        } else if later || modal.should_close() {
            self.scripts_tab.reboot_prompt_open = false;
        }
    }

    /// The picker the queue parks on while Data Transfer waits for a choice.
    fn data_transfer_modal(&mut self, ui: &mut Ui) {
        let tab = &mut self.scripts_tab;
        if !tab.show_data_transfer_ui {
            return;
        }
        let modal = egui::Modal::new(Id::new("scripts_data_transfer")).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.label(
                RichText::new(format!("{} Data Transfer", icons::FOLDER_OPEN))
                    .strong()
                    .color(theme::strong_text(ui)),
            );
            ui.label(
                RichText::new("Select source folders to transfer and a destination.")
                    .color(theme::weak_text(ui)),
            );
            ui.add_space(8.0);

            ui.label(RichText::new("Source folders").strong());
            ScrollArea::vertical()
                .id_salt("scripts_transfer_sources")
                .max_height(220.0)
                .show(ui, |ui| {
                    for (path, size) in &tab.data_transfer_candidates {
                        let mut checked = tab.selected_sources.contains(path);
                        if ui
                            .checkbox(&mut checked, format!("{path} ({size})"))
                            .changed()
                        {
                            if checked {
                                tab.selected_sources.push(path.clone());
                            } else {
                                tab.selected_sources.retain(|p| p != path);
                            }
                        }
                    }
                });

            ui.add_space(8.0);
            ui.label(RichText::new("Destination").strong());
            ui.horizontal(|ui| {
                let shown = tab
                    .selected_destination
                    .as_deref()
                    .unwrap_or("Select destination...");
                ui.label(shown);
                if ui.button(format!("{} Browse...", icons::FOLDER)).clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    tab.selected_destination = Some(path.to_string_lossy().to_string());
                }
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let ready = !tab.selected_sources.is_empty() && tab.selected_destination.is_some();
                let start = Button::new(
                    RichText::new(format!("{} Start transfer", icons::PLAY))
                        .color(theme::success(ui)),
                );
                if ui.add_enabled(ready, start).clicked()
                    && let Some(destination) = tab.selected_destination.clone()
                {
                    return Some(TransferAction::Start(
                        tab.selected_sources.clone(),
                        destination,
                    ));
                }
                if ui
                    .button(
                        RichText::new(format!("{} Cancel", icons::CLOSE)).color(theme::error(ui)),
                    )
                    .clicked()
                {
                    return Some(TransferAction::Cancel);
                }
                None
            })
            .inner
        });

        // A click outside must not discard a half-made selection.
        match modal.inner {
            Some(TransferAction::Start(sources, destination)) => {
                tab.start_data_transfer(sources, destination);
            }
            Some(TransferAction::Cancel) => tab.cancel_data_transfer(),
            None => {}
        }
    }
}

fn scripts_header(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    ui.horizontal_centered(|ui| {
        ui.label(RichText::new("Service #").color(theme::weak_text(ui)));
        ui.add(
            TextEdit::singleline(&mut tab.service_number_input)
                .desired_width(110.0)
                .hint_text("SO number"),
        );
        ui.add_space(8.0);

        let queued = tab.state.queue.len();
        if let Some(name) = tab.stopping_name().map(str::to_owned) {
            ui.label(
                RichText::new(format!(
                    "{} Stopping, waiting for {name}",
                    icons::STATUS_WAIT
                ))
                .color(theme::warn(ui)),
            );
            let abandon = ui
                .small_button("Abandon")
                .on_hover_text("Stop waiting; the script may keep running in the background");
            if abandon.clicked() {
                tab.abandon_stopped_run();
            }
        } else if tab.state.queue.is_running() {
            let stop =
                Button::new(RichText::new(format!("{} Stop", icons::STOP)).color(theme::error(ui)));
            if ui.add(stop).clicked() {
                tab.stop_queue();
            }
        } else {
            let plural = if queued == 1 { "" } else { "s" };
            let run = Button::new(
                RichText::new(format!("{} Run {queued} script{plural}", icons::PLAY))
                    .color(theme::success(ui)),
            );
            if ui.add_enabled(queued > 0, run).clicked() {
                tab.run_queue();
            }
        }

        let (done, total) = tab.state.queue.progress();
        if total > 0 {
            ui.add(
                ProgressBar::new(done as f32 / total as f32)
                    .desired_width(140.0)
                    .text(format!("{done} / {total}")),
            );
        }

        if tab.show_data_transfer_ui {
            ui.label(
                RichText::new(format!(
                    "{} Waiting for the Data Transfer selection",
                    icons::STATUS_WAIT
                ))
                .color(theme::warn(ui)),
            );
        } else if let Some(name) = tab.current_script_name.as_deref() {
            ui.label(
                RichText::new(format!("{} {name}", icons::STATUS_WAIT)).color(theme::info(ui)),
            );
        }

        if let Some((current, total)) = tab.download_progress
            && total > 0
        {
            let fraction = current as f32 / total as f32;
            ui.add(
                ProgressBar::new(fraction)
                    .desired_width(120.0)
                    .text(format!("{} {:.0}%", icons::DOWNLOAD, fraction * 100.0)),
            );
        }
    });
}

fn scripts_body(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    let wide = ui.available_width() >= THREE_COLUMN_MIN_WIDTH;
    let mut builder = StripBuilder::new(ui)
        .size(Size::exact(CATALOG_WIDTH))
        .size(Size::remainder().at_least(240.0));
    if wide {
        builder = builder.size(Size::relative(0.38).at_least(280.0));
    }
    builder.horizontal(|mut strip| {
        strip.cell(|ui| catalog_column(ui, tab));
        if wide {
            strip.cell(|ui| queue_column(ui, tab));
            strip.cell(|ui| log_panel(ui, tab));
        } else {
            strip.strip(|builder| {
                builder
                    .size(Size::relative(0.55))
                    .size(Size::remainder())
                    .vertical(|mut strip| {
                        strip.cell(|ui| queue_column(ui, tab));
                        strip.cell(|ui| log_panel(ui, tab));
                    });
            });
        }
    });
}

fn catalog_column(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    ui.add(
        TextEdit::singleline(&mut tab.search)
            .hint_text(format!("{} Filter scripts", icons::SEARCH))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);

    if let Some(preset) = CATALOG.preset(STANDARD_PRESET) {
        let text = RichText::new(format!("{} {}", icons::STAR, preset.name))
            .strong()
            .color(theme::accent(ui));
        let button = Button::new(text).min_size(vec2(ui.available_width(), 28.0));
        if ui.add(button).on_hover_text(&preset.summary).clicked() {
            let ids: Vec<String> = preset
                .scripts
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect();
            tab.queue_ids(&preset.name, &ids);
        }
    }

    let selected = tab
        .state
        .categories
        .values()
        .flatten()
        .filter(|s| s.is_selected())
        .count();
    let add = Button::new(format!("{} Add {selected} selected", icons::PLUS))
        .min_size(vec2(ui.available_width(), 24.0));
    if ui.add_enabled(selected > 0, add).clicked() {
        tab.queue_selected();
    }
    ui.add_space(6.0);

    let terms: Vec<String> = tab
        .search
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    ScrollArea::vertical()
        .id_salt("scripts_catalog")
        .auto_shrink(false)
        .show(ui, |ui| {
            for category in CATEGORY_ORDER.iter() {
                category_card(ui, tab, category, &terms);
            }
        });
}

fn category_card(
    ui: &mut Ui,
    tab: &mut EguiScriptsTab,
    category: &ScriptCategory,
    terms: &[String],
) {
    let Some(scripts) = tab.state.categories.get(category) else {
        return;
    };
    let rows: Vec<(usize, String, String, bool)> = scripts
        .iter()
        .enumerate()
        .filter(|(_, s)| matches_terms(s, terms))
        .map(|(i, s)| (i, s.name.clone(), s.description.clone(), s.is_selected()))
        .collect();
    if rows.is_empty() {
        return;
    }
    let picked = scripts.iter().filter(|s| s.is_selected()).count();
    let total = scripts.len();
    let searching = !terms.is_empty();
    let expanded = searching
        || tab
            .state
            .category_expanded
            .get(category)
            .copied()
            .unwrap_or(true);

    glass_card::group(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let chevron = if expanded {
                icons::CHEV_OPEN
            } else {
                icons::CHEV_CLOSED
            };
            let title = RichText::new(format!(
                "{chevron} {} {}",
                category_glyph(category),
                category_display_name(category)
            ))
            .strong()
            .color(theme::strong_text(ui));
            if ui.add(Button::new(title).frame(false)).clicked() && !searching {
                tab.state
                    .category_expanded
                    .insert(category.clone(), !expanded);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !searching {
                    let label = if picked > 0 { "None" } else { "All" };
                    if ui.small_button(label).clicked() {
                        if picked > 0 {
                            tab.state.deselect_category(category);
                        } else {
                            tab.state.select_category(category);
                        }
                    }
                }
                ui.label(
                    RichText::new(format!("{picked}/{total}"))
                        .small()
                        .color(theme::weak_text(ui)),
                );
            });
        });

        if !expanded {
            return;
        }
        let weak = theme::weak_text(ui);
        let check = theme::success(ui);
        for (index, name, summary, selected) in rows {
            let response = selectable_card(ui, ("scripts_catalog_row", &name), selected, |ui| {
                ui.horizontal(|ui| {
                    let (glyph, color) = if selected {
                        (icons::STATUS_ON, check)
                    } else {
                        (icons::STATUS_IDLE, weak)
                    };
                    ui.label(icons::icon_colored(glyph, color));
                    ui.add(Label::new(RichText::new(&name).strong()).truncate());
                });
                if !summary.is_empty() {
                    ui.add(Label::new(RichText::new(&summary).small().color(weak)).truncate());
                }
            })
            .response;
            if response.clicked() {
                if let Some(script) = tab
                    .state
                    .categories
                    .get_mut(category)
                    .and_then(|list| list.get_mut(index))
                {
                    script.toggle_selection();
                }
                tab.focus = Some(ScriptFocus::Catalog(name));
            }
        }
    });
}

fn queue_column(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    let has_focus = tab.focus.is_some();
    let mut builder = StripBuilder::new(ui).size(Size::remainder().at_least(120.0));
    if has_focus {
        builder = builder.size(Size::relative(0.5).at_least(160.0));
    }
    builder.vertical(|mut strip| {
        strip.cell(|ui| queue_panel(ui, tab));
        if has_focus {
            strip.cell(|ui| detail_panel(ui, tab));
        }
    });
}

fn queue_panel(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    let running = tab.state.queue.is_running();
    ui.horizontal(|ui| {
        ui.label(icons::icon_colored(
            icons::LIST,
            theme::accent_secondary(ui),
        ));
        ui.label(
            RichText::new("Queue")
                .strong()
                .color(theme::strong_text(ui)),
        );
        ui.label(
            RichText::new(tab.state.queue.len().to_string())
                .small()
                .color(theme::weak_text(ui)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear = Button::new(format!("{} Clear", icons::TRASH)).small();
            if ui
                .add_enabled(!running && !tab.state.queue.is_empty(), clear)
                .clicked()
            {
                tab.state.queue.clear();
                tab.outcomes.clear();
                if matches!(tab.focus, Some(ScriptFocus::Queued(_))) {
                    tab.focus = None;
                }
            }
        });
    });
    glass_card::hairline(ui);

    if tab.state.queue.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(
                RichText::new("Queue is empty")
                    .italics()
                    .color(theme::weak_text(ui)),
            );
            ui.label(
                RichText::new("Pick scripts on the left, or use Standard Tune-up")
                    .small()
                    .color(theme::weak_text(ui)),
            );
        });
        return;
    }

    let rows: Vec<QueueRow> = tab
        .state
        .queue
        .items()
        .iter()
        .map(|q| QueueRow {
            token: q.run_token,
            order: q.order,
            name: q.script.name.clone(),
            status: q.script.status,
        })
        .collect();
    let len = rows.len();
    let mut action = None;

    ScrollArea::vertical()
        .id_salt("scripts_queue")
        .auto_shrink(false)
        .show(ui, |ui| {
            for (i, row) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let up = Button::new(icons::UP).small();
                        if ui.add_enabled(!running && i > 0, up).clicked() {
                            action = Some(QueueAction::Move(i, i - 1));
                        }
                        let down = Button::new(icons::ARROW_DOWN).small();
                        if ui.add_enabled(!running && i + 1 < len, down).clicked() {
                            action = Some(QueueAction::Move(i, i + 1));
                        }
                    });

                    // Width left after the remove button.
                    let card_width = (ui.available_width() - 28.0).max(80.0);
                    let focused = tab.focus == Some(ScriptFocus::Queued(row.token));
                    let (label, color) =
                        status_badge(ui, &row.status, tab.outcomes.get(&row.token));
                    let clicked = ui
                        .allocate_ui_with_layout(
                            vec2(card_width, 0.0),
                            Layout::top_down_justified(Align::LEFT),
                            |ui| {
                                selectable_card(
                                    ui,
                                    ("scripts_queue_row", row.token),
                                    focused,
                                    |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("#{}", row.order + 1))
                                                    .monospace()
                                                    .color(theme::weak_text(ui)),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    badge(ui, label, color);
                                                    ui.add(
                                                        Label::new(
                                                            RichText::new(&row.name).strong(),
                                                        )
                                                        .truncate(),
                                                    );
                                                },
                                            );
                                        });
                                    },
                                )
                                .response
                                .clicked()
                            },
                        )
                        .inner;
                    if clicked {
                        action = Some(QueueAction::Focus(row.token));
                    }

                    let removable = !(running && row.status == ScriptStatus::Running);
                    let remove = Button::new(icons::CLOSE).small();
                    if ui
                        .add_enabled(removable, remove)
                        .on_hover_text("Remove from the queue")
                        .clicked()
                    {
                        action = Some(QueueAction::Remove(row.token));
                    }
                });
            }
        });

    match action {
        Some(QueueAction::Move(from, to)) => tab.state.queue.move_item(from, to),
        Some(QueueAction::Remove(token)) => {
            tab.state.queue.remove(token);
            tab.outcomes.remove(&token);
            if tab.focus == Some(ScriptFocus::Queued(token)) {
                tab.focus = None;
            }
        }
        Some(QueueAction::Focus(token)) => tab.focus = Some(ScriptFocus::Queued(token)),
        None => {}
    }
}

fn detail_panel(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    let Some(focus) = tab.focus.clone() else {
        return;
    };
    let (name, status, token) = match &focus {
        ScriptFocus::Catalog(name) => (name.clone(), None, None),
        ScriptFocus::Queued(token) => {
            match tab
                .state
                .queue
                .items()
                .iter()
                .find(|q| q.run_token == *token)
            {
                Some(q) => (q.script.name.clone(), Some(q.script.status), Some(*token)),
                None => {
                    tab.focus = None;
                    return;
                }
            }
        }
    };
    let def = CATALOG
        .id_for_legacy_name(&name)
        .and_then(|id| CATALOG.get(id));
    let outcome = token.and_then(|t| tab.outcomes.get(&t));

    let mut close = false;
    ScrollArea::vertical()
        .id_salt("scripts_detail")
        .auto_shrink(false)
        .show(ui, |ui| {
            glass_card::group(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(icons::icon_colored(
                        icons::INFO,
                        theme::accent_secondary(ui),
                    ));
                    ui.label(RichText::new(&name).strong().color(theme::strong_text(ui)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button(icons::CLOSE)
                            .on_hover_text("Close")
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                glass_card::hairline(ui);

                if let Some(def) = def {
                    definition_rows(ui, def);
                }
                if let Some(status) = status {
                    ui.add_space(6.0);
                    glass_card::hairline(ui);
                    kv_row(ui, "Status", &status.to_string());
                    if let Some(outcome) = outcome {
                        outcome_rows(ui, outcome);
                    }
                }
            });
        });
    if close {
        tab.focus = None;
    }
}

fn definition_rows(ui: &mut Ui, def: &ScriptDef) {
    kv_row(ui, "Category", category_display_name(&def.category()));
    kv_row(ui, "Time limit", &format_budget(def.timeout_secs));
    let elevation = match def.elevation {
        Elevation::Admin => "Administrator",
        Elevation::None => "Not required",
    };
    kv_row(ui, "Elevation", elevation);
    if !def.requires.is_empty() {
        let needs: Vec<&str> = def.requires.iter().map(requirement_label).collect();
        kv_row(ui, "Needs", &needs.join(", "));
    }
    match def.reboot {
        RebootHint::Never => {}
        RebootHint::Maybe => kv_row(ui, "Reboot", "Sometimes"),
        RebootHint::Always => kv_row(ui, "Reboot", "Always"),
    }
    if !def.runs.is_empty() {
        kv_row(ui, "Runs", &format!("{} scripts in order", def.runs.len()));
    }

    let about = if def.detail.trim().is_empty() {
        &def.summary
    } else {
        &def.detail
    };
    if !about.trim().is_empty() {
        ui.add_space(4.0);
        expandable_text(ui, def.id.as_str(), about, 3);
    }

    let criteria = [
        ("PASS", theme::success(ui), def.pass.as_deref()),
        ("WARN", theme::warn(ui), def.warn.as_deref()),
        ("FAIL", theme::error(ui), def.fail.as_deref()),
    ];
    if criteria.iter().any(|(_, _, text)| text.is_some()) {
        ui.add_space(4.0);
        for (label, color, text) in criteria {
            if let Some(text) = text {
                ui.horizontal_top(|ui| {
                    badge(ui, label, color);
                    ui.add(Label::new(text).wrap());
                });
            }
        }
    }
}

fn outcome_rows(ui: &mut Ui, outcome: &ScriptOutcome) {
    let (label, color) = result_badge(ui, &outcome.result);
    ui.horizontal_top(|ui| {
        badge(ui, label, color);
        ui.add(Label::new(outcome.result.message()).wrap());
    });
    kv_row(
        ui,
        "Duration",
        &format!("{:.1}s", outcome.duration.as_secs_f32()),
    );
    if let Some(code) = outcome.exit_code {
        kv_row(ui, "Exit code", &code.to_string());
    }
    if let Some(run_id) = &outcome.run_id {
        kv_row(ui, "Run id", run_id);
    }
    if outcome.reboot_recommended {
        kv_row(ui, "Reboot", "Recommended");
    }
}

fn log_panel(ui: &mut Ui, tab: &mut EguiScriptsTab) {
    ui.horizontal(|ui| {
        ui.label(icons::icon_colored(
            icons::SCROLL,
            theme::accent_secondary(ui),
        ));
        ui.label(RichText::new("Log").strong().color(theme::strong_text(ui)));
        let filters = [
            ("All", None),
            ("Info", Some(LogLevel::Info)),
            ("OK", Some(LogLevel::Success)),
            ("Warn", Some(LogLevel::Warning)),
            ("Error", Some(LogLevel::Error)),
        ];
        for (label, level) in filters {
            ui.framed_selectable_value(&mut tab.log_filter, level, label);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button(icons::TRASH)
                .on_hover_text("Clear the log")
                .clicked()
            {
                tab.state.clear_logs();
            }
            if ui
                .small_button(icons::COPY)
                .on_hover_text("Copy the shown lines")
                .clicked()
            {
                ui.ctx().copy_text(export_log(tab));
            }
            ui.checkbox(&mut tab.auto_scroll_logs, "Follow");
        });
    });
    glass_card::hairline(ui);

    let filter = tab.log_filter;
    let rows: Vec<usize> = tab
        .state
        .logs()
        .iter()
        .enumerate()
        .filter(|(_, e)| filter.is_none_or(|level| level == e.level))
        .map(|(i, _)| i)
        .collect();
    if rows.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(
                RichText::new("No log entries yet")
                    .italics()
                    .color(theme::weak_text(ui)),
            );
        });
        return;
    }

    let row_height = ui
        .spacing()
        .interact_size
        .y
        .max(ui.text_style_height(&TextStyle::Body));
    ScrollArea::vertical()
        .id_salt("scripts_log")
        .auto_shrink(false)
        .stick_to_bottom(tab.auto_scroll_logs)
        .show_rows(ui, row_height, rows.len(), |ui, range| {
            for &index in &rows[range] {
                let entry = &tab.state.logs()[index];
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), row_height),
                    Layout::left_to_right(Align::Center),
                    |ui| log_row(ui, entry),
                );
            }
        });
}

fn log_row(ui: &mut Ui, entry: &ScriptLogEntry) {
    let color = level_color(ui, entry.level);
    let weak = theme::weak_text(ui);
    let body = TextStyle::Body.resolve(ui.style());
    let small = TextStyle::Small.resolve(ui.style());

    let mut job = LayoutJob::default();
    job.append(
        &entry.timestamp.format("%H:%M:%S").to_string(),
        0.0,
        TextFormat::simple(FontId::monospace(small.size), weak),
    );
    job.append(
        level_glyph(entry.level),
        6.0,
        TextFormat::simple(body.clone(), color),
    );
    job.append(
        &format!("[{}]", entry.script_name),
        6.0,
        TextFormat::simple(small, weak),
    );
    job.append(&entry.message, 6.0, TextFormat::simple(body, color));
    ui.add(Label::new(job).truncate())
        .on_hover_text(&entry.message);
}

fn export_log(tab: &EguiScriptsTab) -> String {
    tab.state
        .logs()
        .iter()
        .filter(|e| tab.log_filter.is_none_or(|level| level == e.level))
        .map(|e| {
            format!(
                "{} [{}] [{}] {}",
                e.timestamp.format("%H:%M:%S"),
                level_label(e.level),
                e.script_name,
                e.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn matches_terms(script: &ScriptItem, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let haystack = format!("{} {}", script.name, script.description).to_lowercase();
    terms.iter().all(|term| haystack.contains(term.as_str()))
}

fn status_badge(
    ui: &Ui,
    status: &ScriptStatus,
    outcome: Option<&ScriptOutcome>,
) -> (&'static str, Color32) {
    if let Some(outcome) = outcome {
        return result_badge(ui, &outcome.result);
    }
    match status {
        ScriptStatus::Pending => ("QUEUED", theme::weak_text(ui)),
        ScriptStatus::Running => ("RUNNING", theme::info(ui)),
        ScriptStatus::Completed => ("DONE", theme::success(ui)),
        ScriptStatus::Failed => ("FAILED", theme::error(ui)),
        ScriptStatus::Skipped => ("SKIPPED", theme::warn(ui)),
    }
}

fn result_badge(ui: &Ui, result: &ScriptResult) -> (&'static str, Color32) {
    match result {
        ScriptResult::Success(_) => ("PASS", theme::success(ui)),
        ScriptResult::Warning(_) => ("WARN", theme::warn(ui)),
        ScriptResult::Error(_) => ("FAIL", theme::error(ui)),
        ScriptResult::Skipped(_) => ("SKIPPED", theme::weak_text(ui)),
    }
}

fn level_color(ui: &Ui, level: LogLevel) -> Color32 {
    match level {
        LogLevel::Info => ui.visuals().text_color(),
        LogLevel::Success => theme::success(ui),
        LogLevel::Warning => theme::warn(ui),
        LogLevel::Error => theme::error(ui),
    }
}

fn level_glyph(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => icons::INFO,
        LogLevel::Success => icons::STATUS_ON,
        LogLevel::Warning => icons::STATUS_WARN,
        LogLevel::Error => icons::STATUS_ERR,
    }
}

fn level_label(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "INFO",
        LogLevel::Success => "OK",
        LogLevel::Warning => "WARN",
        LogLevel::Error => "ERR",
    }
}

fn requirement_label(requirement: &Requirement) -> &'static str {
    match requirement {
        Requirement::ServiceNumber => "Service number",
        Requirement::CustomerEmail => "Customer email",
        Requirement::Internet => "Internet",
        Requirement::Gpu => "GPU",
    }
}

fn category_glyph(category: &ScriptCategory) -> &'static str {
    match category {
        ScriptCategory::Tuneup => icons::WRENCH,
        ScriptCategory::Informational => icons::INFO,
        ScriptCategory::JunkwareRemoval => icons::TRASH,
        ScriptCategory::StressTests => icons::FLASK,
        ScriptCategory::UserScripts(_) => icons::FILE_TEXT,
        ScriptCategory::Custom(_) => icons::GEAR,
    }
}

fn format_budget(secs: u64) -> String {
    if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("up to {} h", secs / 3600)
    } else if secs >= 60 {
        format!("up to {} min", secs / 60)
    } else {
        format!("up to {secs} s")
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn item(name: &str, description: &str) -> ScriptItem {
        ScriptItem::new(name, ScriptCategory::Tuneup).with_description(description)
    }

    #[test]
    fn every_search_term_must_match() {
        let script = item(
            "Disable BitLocker",
            "Detect and disable BitLocker encryption",
        );
        assert!(matches_terms(&script, &[]));
        assert!(matches_terms(&script, &["bitlocker".into()]));
        assert!(matches_terms(
            &script,
            &["disable".into(), "encryption".into()]
        ));
        assert!(!matches_terms(
            &script,
            &["disable".into(), "webroot".into()]
        ));
    }

    #[test]
    fn budgets_read_in_the_largest_whole_unit() {
        assert_eq!(format_budget(600), "up to 10 min");
        assert_eq!(format_budget(3600), "up to 1 h");
        assert_eq!(format_budget(5400), "up to 90 min");
        assert_eq!(format_budget(45), "up to 45 s");
    }
}

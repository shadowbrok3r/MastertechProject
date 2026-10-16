//! The Scripts tab: catalog on the left, queue and details in the middle, run log on the right.

use displays::scripts::catalog::{CATALOG, Elevation, RebootHint, Requirement, ScriptDef};
use displays::scripts::executor::{ScriptOutcome, ScriptResult};
use displays::scripts::{
    CATEGORY_ORDER, LogLevel, ScriptCategory, ScriptItem, ScriptLogEntry, ScriptStatus,
    category_display_name,
};
use displays::ui_tools::controls::{
    self, BarKind, ButtonKind, IconBox, IconButton, Metrics, ModalWidth, Role, SpinnerSize,
    TextButton, TextField,
};
use displays::ui_tools::framed_controls::selectable_card;
use displays::ui_tools::icons::svg::{self, Icon};
use displays::ui_tools::info_card::{expandable_text, kv_row};
use displays::ui_tools::{glass_card, icons, theme};
use eframe::egui::text::LayoutJob;
use eframe::egui::{
    Align, Color32, FontId, Id, Label, Layout, RichText, ScrollArea, TextFormat, TextStyle, Ui,
    vec2,
};
use egui_extras::{Size, StripBuilder};

use super::EguiScriptsTab;
use crate::app_state::MastertechContext;

/// Below this width the log stacks under the queue instead of taking its own column.
const THREE_COLUMN_MIN_WIDTH: f32 = 1000.0;
const CATALOG_WIDTH: f32 = 280.0;
const STANDARD_PRESET: &str = "standard-tuneup";
/// Width of the service-number field.
const SERVICE_FIELD_W: f32 = 130.0;
/// Width of the queue progress bar.
const QUEUE_PROGRESS_W: f32 = 140.0;
/// Width of the download progress bar.
const DOWNLOAD_PROGRESS_W: f32 = 120.0;
/// Tallest the transfer source list grows before it scrolls.
const SOURCES_MAX_H: f32 = 220.0;
/// Smallest width a dialog footer button takes.
const DIALOG_BUTTON_MIN_W: f32 = 88.0;

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
        self.reboot_modal(ui);
        self.data_transfer_modal(ui);

        let header = Metrics::get(ui.ctx()).bar_h;
        StripBuilder::new(ui)
            .size(Size::exact(header))
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| scripts_header(ui, &mut self.scripts_tab));
                strip.cell(|ui| scripts_body(ui, &mut self.scripts_tab));
            });
    }

    /// Copies the ticket's service number in only when the ticket changes, so the field stays editable.
    pub(crate) fn seed_script_context(&mut self) {
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
        let modal = controls::modal(
            ui.ctx(),
            Id::unique("scripts_reboot_prompt"),
            svg::ARROWS_CLOCKWISE,
            "Reboot required",
            ModalWidth::Narrow,
            &[(Role::Primary, "Reboot now"), (Role::Cancel, "Later")],
            |ui| {
                ui.label("Webroot was re-keyed over an existing install.");
                ui.label(
                    "A reboot finalizes the new device identity. MasterTech will relaunch after login.",
                );
            },
        );

        if modal.clicked == Some(0) {
            #[cfg(target_os = "windows")]
            crate::utilities::windows::reboot::spawn_reboot_with_relaunch(
                false,
                "Mastertech reboot to finalize Webroot activation",
            );
            self.scripts_tab.reboot_prompt_open = false;
        } else if modal.should_close {
            self.scripts_tab.reboot_prompt_open = false;
        }
    }

    /// The picker the queue parks on while Data Transfer waits for a choice.
    fn data_transfer_modal(&mut self, ui: &mut Ui) {
        let tab = &mut self.scripts_tab;
        if !tab.show_data_transfer_ui {
            return;
        }
        let modal = controls::modal(
            ui.ctx(),
            Id::unique("scripts_data_transfer"),
            svg::FOLDER_OPEN,
            "Data Transfer",
            ModalWidth::Regular,
            &[],
            |ui| {
                let m = Metrics::get(ui.ctx());
                ui.label(
                    RichText::new("Select source folders to transfer and a destination.")
                        .color(theme::weak_text(ui)),
                );
                ui.add_space(m.gap.y);

                controls::section_label(ui, "Source folders");
                ScrollArea::vertical()
                    .id_salt("scripts_transfer_sources")
                    .max_height(SOURCES_MAX_H)
                    .show(ui, |ui| {
                        for (path, size) in &tab.data_transfer_candidates {
                            let mut checked = tab.selected_sources.contains(path);
                            if controls::checkbox(ui, &mut checked, &format!("{path} ({size})"))
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

                ui.add_space(m.gap.y);
                controls::section_label(ui, "Destination");
                ui.horizontal(|ui| {
                    let shown = tab
                        .selected_destination
                        .as_deref()
                        .unwrap_or("Select destination...");
                    controls::icon_label(ui, svg::FOLDER, shown);
                    if controls::icon_text_button(ui, svg::FOLDER_OPEN, "Browse...").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        tab.selected_destination = Some(path.to_string_lossy().to_string());
                    }
                });

                ui.add_space(m.gap.y);
                controls::bar(ui, BarKind::Footer, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let cancel = TextButton::new("Cancel")
                            .icon(svg::X)
                            .prominent(true)
                            .min_width(DIALOG_BUTTON_MIN_W)
                            .show(ui);
                        if cancel.clicked() {
                            return Some(TransferAction::Cancel);
                        }
                        let ready =
                            !tab.selected_sources.is_empty() && tab.selected_destination.is_some();
                        let start = TextButton::new("Start transfer")
                            .icon(svg::PLAY)
                            .kind(ButtonKind::Primary)
                            .prominent(true)
                            .min_width(DIALOG_BUTTON_MIN_W)
                            .enabled(ready)
                            .show(ui);
                        if start.clicked()
                            && let Some(destination) = tab.selected_destination.clone()
                        {
                            return Some(TransferAction::Start(
                                tab.selected_sources.clone(),
                                destination,
                            ));
                        }
                        None
                    })
                    .inner
                })
            },
        );

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
    controls::bar(ui, BarKind::Tool, |ui| {
        TextField::new(&mut tab.service_number_input)
            .hint("SO number")
            .icon(svg::HASH)
            .width(SERVICE_FIELD_W)
            .id_salt("scripts.service_number")
            .show(ui)
            .on_hover_text("Service #");

        let queued = tab.state.queue.len();
        if let Some(name) = tab.stopping_name().map(str::to_owned) {
            controls::spinner(ui, SpinnerSize::Sm);
            ui.label(RichText::new(format!("Stopping, waiting for {name}")).color(theme::warn(ui)));
            let abandon = TextButton::new("Abandon")
                .icon(svg::X_CIRCLE)
                .kind(ButtonKind::Ghost)
                .show(ui)
                .on_hover_text("Stop waiting; the script may keep running in the background");
            if abandon.clicked() {
                tab.abandon_stopped_run();
            }
        } else if tab.state.queue.is_running() {
            let stop = TextButton::new("Stop")
                .icon(svg::STOP)
                .kind(ButtonKind::Danger)
                .show(ui);
            if stop.clicked() {
                tab.stop_queue();
            }
        } else {
            let plural = if queued == 1 { "" } else { "s" };
            let label = format!("Run {queued} script{plural}");
            let run = TextButton::new(&label)
                .icon(svg::PLAY)
                .kind(ButtonKind::Primary)
                .enabled(queued > 0)
                .show(ui);
            if run.clicked() {
                tab.run_queue();
            }
        }

        let (done, total) = tab.state.queue.progress();
        if total > 0 {
            controls::ProgressBar::new(done as f32 / total as f32)
                .width(QUEUE_PROGRESS_W)
                .text(&format!("{done} / {total}"))
                .show(ui);
        }

        if tab.show_data_transfer_ui {
            controls::status_label(
                ui,
                svg::HOURGLASS,
                "Waiting for the Data Transfer selection",
                theme::warn(ui),
            );
        } else if let Some(name) = tab.current_script_name.as_deref() {
            controls::spinner(ui, SpinnerSize::Sm);
            ui.label(RichText::new(name).color(theme::info(ui)));
        }

        if let Some((current, total)) = tab.download_progress
            && total > 0
        {
            let fraction = current as f32 / total as f32;
            controls::ProgressBar::new(fraction)
                .width(DOWNLOAD_PROGRESS_W)
                .text(&format!("{} {:.0}%", icons::DOWNLOAD, fraction * 100.0))
                .show(ui);
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
    let m = Metrics::get(ui.ctx());
    let full = ui.available_width();
    controls::search_field(ui, &mut tab.search, "Filter scripts", full);
    ui.add_space(m.gap.y);

    if let Some(preset) = CATALOG.preset(STANDARD_PRESET) {
        let button = TextButton::new(&preset.name)
            .icon(svg::STAR)
            .kind(ButtonKind::Primary)
            .prominent(true)
            .min_width(full)
            .show(ui);
        if button.on_hover_text(&preset.summary).clicked() {
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
    let label = format!("Add {selected} selected");
    let add = TextButton::new(&label)
        .icon(svg::PLUS)
        .min_width(full)
        .enabled(selected > 0)
        .show(ui);
    if add.clicked() {
        tab.queue_selected();
    }
    ui.add_space(m.gap.y);

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
                svg::CARET_DOWN
            } else {
                svg::CARET_RIGHT
            };
            let tip = if expanded { "Collapse" } else { "Expand" };
            let toggled = IconButton::new(chevron, IconBox::Sm)
                .tip(tip)
                .show(ui)
                .clicked();
            let title = TextButton::new(category_display_name(category))
                .icon(category_icon(category))
                .kind(ButtonKind::Ghost)
                .show(ui)
                .clicked();
            if (toggled || title) && !searching {
                tab.state
                    .category_expanded
                    .insert(category.clone(), !expanded);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !searching {
                    let (icon, label) = if picked > 0 {
                        (svg::SQUARE, "None")
                    } else {
                        (svg::CHECK_SQUARE, "All")
                    };
                    let pick = TextButton::new(label)
                        .icon(icon)
                        .kind(ButtonKind::Ghost)
                        .show(ui);
                    if pick.clicked() {
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
        let icon_size = Metrics::get(ui.ctx()).icon_sm;
        for (index, name, summary, selected) in rows {
            let response = selectable_card(ui, ("scripts_catalog_row", &name), selected, |ui| {
                ui.horizontal(|ui| {
                    let (icon, color) = if selected {
                        (svg::CHECK_CIRCLE, check)
                    } else {
                        (svg::CIRCLE, weak)
                    };
                    ui.add(icon.image(icon_size).tint(color));
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
    let m = Metrics::get(ui.ctx());
    let running = tab.state.queue.is_running();
    controls::bar(ui, BarKind::Header, |ui| {
        ui.add(
            svg::QUEUE
                .image(m.icon_md)
                .tint(theme::accent_secondary(ui)),
        );
        ui.label(
            RichText::new("Queue")
                .strong()
                .color(theme::strong_text(ui)),
        );
        controls::badge(ui, &tab.state.queue.len().to_string(), theme::weak_text(ui));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear = TextButton::new("Clear")
                .icon(svg::TRASH)
                .kind(ButtonKind::Ghost)
                .enabled(!running && !tab.state.queue.is_empty())
                .show(ui);
            if clear.clicked() {
                tab.state.queue.clear();
                tab.outcomes.clear();
                if matches!(tab.focus, Some(ScriptFocus::Queued(_))) {
                    tab.focus = None;
                }
            }
        });
    });

    if tab.state.queue.is_empty() {
        controls::empty_state(
            ui,
            svg::QUEUE,
            "Queue is empty",
            "Pick scripts on the left, or use Standard Tune-up",
            &[],
        );
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
                        let up = IconButton::new(svg::CARET_UP, IconBox::Sm)
                            .tip("Move up")
                            .enabled(!running && i > 0)
                            .show(ui);
                        if up.clicked() {
                            action = Some(QueueAction::Move(i, i - 1));
                        }
                        let down = IconButton::new(svg::CARET_DOWN, IconBox::Sm)
                            .tip("Move down")
                            .enabled(!running && i + 1 < len)
                            .show(ui);
                        if down.clicked() {
                            action = Some(QueueAction::Move(i, i + 1));
                        }
                    });

                    // Width left after the remove button.
                    let card_width =
                        (ui.available_width() - IconBox::Sm.side(&m) - ui.spacing().item_spacing.x)
                            .max(80.0);
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
                                                    controls::badge(ui, label, color);
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
                    let remove = IconButton::new(svg::X, IconBox::Sm)
                        .tip("Remove from the queue")
                        .enabled(removable)
                        .show(ui);
                    if remove.clicked() {
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
                let m = Metrics::get(ui.ctx());
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(svg::INFO.image(m.icon_md).tint(theme::accent_secondary(ui)));
                    ui.label(RichText::new(&name).strong().color(theme::strong_text(ui)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if controls::icon_button(ui, svg::X, IconBox::Sm, false, "Close").clicked()
                        {
                            close = true;
                        }
                    });
                });
                controls::hairline(ui);

                if let Some(def) = def {
                    definition_rows(ui, def);
                }
                if let Some(status) = status {
                    ui.add_space(m.gap.y);
                    controls::hairline(ui);
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
    let m = Metrics::get(ui.ctx());
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
        ui.add_space(m.gap.y / 2.0);
        expandable_text(ui, def.id.as_str(), about, 3);
    }

    let criteria = [
        ("PASS", theme::success(ui), def.pass.as_deref()),
        ("WARN", theme::warn(ui), def.warn.as_deref()),
        ("FAIL", theme::error(ui), def.fail.as_deref()),
    ];
    if criteria.iter().any(|(_, _, text)| text.is_some()) {
        ui.add_space(m.gap.y / 2.0);
        for (label, color, text) in criteria {
            if let Some(text) = text {
                ui.horizontal_top(|ui| {
                    controls::badge(ui, label, color);
                    ui.add(Label::new(text).wrap());
                });
            }
        }
    }
}

fn outcome_rows(ui: &mut Ui, outcome: &ScriptOutcome) {
    let (label, color) = result_badge(ui, &outcome.result);
    ui.horizontal_top(|ui| {
        controls::badge(ui, label, color);
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
    let m = Metrics::get(ui.ctx());
    controls::bar(ui, BarKind::Header, |ui| {
        ui.add(
            svg::SCROLL
                .image(m.icon_md)
                .tint(theme::accent_secondary(ui)),
        );
        ui.label(RichText::new("Log").strong().color(theme::strong_text(ui)));
        let filters = [
            (svg::LIST, "All", None),
            (svg::INFO, "Info", Some(LogLevel::Info)),
            (svg::CHECK_CIRCLE, "OK", Some(LogLevel::Success)),
            (svg::WARNING, "Warn", Some(LogLevel::Warning)),
            (svg::X_CIRCLE, "Error", Some(LogLevel::Error)),
        ];
        for (icon, label, level) in filters {
            if controls::pill(ui, icon, label, tab.log_filter == level).clicked() {
                tab.log_filter = level;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if controls::icon_button(ui, svg::TRASH, IconBox::Md, false, "Clear the log").clicked()
            {
                tab.state.clear_logs();
            }
            if controls::icon_button(ui, svg::COPY, IconBox::Md, false, "Copy the shown lines")
                .clicked()
            {
                ui.ctx().copy_text(export_log(tab));
            }
            controls::toggle(ui, &mut tab.auto_scroll_logs, "Follow");
        });
    });

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
        controls::empty_state(ui, svg::SCROLL, "No log entries yet", "", &[]);
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

fn category_icon(category: &ScriptCategory) -> Icon {
    match category {
        ScriptCategory::Tuneup => svg::WRENCH,
        ScriptCategory::Informational => svg::INFO,
        ScriptCategory::JunkwareRemoval => svg::TRASH,
        ScriptCategory::StressTests => svg::FLASK,
        ScriptCategory::UserScripts(_) => svg::FILE_TEXT,
        ScriptCategory::Custom(_) => svg::GEAR,
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

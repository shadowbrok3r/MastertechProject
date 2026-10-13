use crate::ui_tools::metrics::Metrics;
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{self, Align, Label, Layout, RichText, ScrollArea, Ui};

use crate::ui_tools::controls::{self, BarKind, ButtonKind, SpinnerSize, TextButton, TextField};
use crate::ui_tools::list_row::{Lead, ListRow};
use crate::ui_tools::{glass_card, icons::svg, theme};
use crate::{Cmd, PlatformSpawner, RemoteScriptItem, RemoteScriptStatus, Spawner};

/// Log lines retained in the viewer; the oldest are dropped past this.
const MAX_LOG_LINES: usize = 5_000;

struct ScriptCategoryGroup {
    name: String,
    items: Vec<ScriptCheckItem>,
}

struct ScriptCheckItem {
    name: String,
    category: String,
    checked: bool,
    status: Option<RemoteScriptStatus>,
    content: Option<String>,
}

pub struct RemoteScriptsViewer {
    categories: Vec<ScriptCategoryGroup>,
    selected_category: usize,
    pub service_number: String,
    pub customer_email: String,
    pub log_messages: Vec<String>,
    pub running: bool,
    pub loading: bool,
    /// A completed script logged the reboot-recommended marker.
    pub reboot_recommended: bool,
    auto_scroll: bool,
    custom_scripts_rx: Receiver<Vec<(String, String)>>,
    custom_scripts_tx: Sender<Vec<(String, String)>>,
    custom_scripts_loaded: bool,
}

impl RemoteScriptsViewer {
    pub fn new() -> Self {
        let (custom_scripts_tx, custom_scripts_rx) = crossbeam::channel::unbounded();
        Self {
            categories: Vec::new(),
            selected_category: 0,
            service_number: String::new(),
            customer_email: String::new(),
            log_messages: Vec::new(),
            running: false,
            loading: false,
            reboot_recommended: false,
            auto_scroll: true,
            custom_scripts_rx,
            custom_scripts_tx,
            custom_scripts_loaded: false,
        }
    }

    pub fn set_script_list(&mut self, categories: Vec<(String, Vec<RemoteScriptItem>)>) {
        self.categories = categories
            .into_iter()
            .map(|(name, items)| ScriptCategoryGroup {
                name,
                items: items
                    .into_iter()
                    .map(|item| ScriptCheckItem {
                        category: item.category,
                        name: item.name,
                        checked: false,
                        status: None,
                        content: item.content,
                    })
                    .collect(),
            })
            .collect();
        self.loading = false;
        self.custom_scripts_loaded = false;
    }

    pub fn load_custom_scripts(&self, bucket_name: &str) {
        let tx = self.custom_scripts_tx.clone();
        let bucket = bucket_name.to_string();
        PlatformSpawner::spawn(async move {
            use database::schema::file_storage;
            let scripts_prefix = "Scripts";
            match file_storage::list_files(&bucket, scripts_prefix).await {
                Ok(entries) => {
                    let mut scripts = Vec::new();
                    for entry in entries {
                        if entry.is_directory {
                            continue;
                        }
                        let name = entry.filename();
                        if name.is_empty() {
                            continue;
                        }
                        let is_script = name.ends_with(".ps1")
                            || name.ends_with(".bat")
                            || name.ends_with(".cmd");
                        if !is_script {
                            continue;
                        }
                        let path = entry.path();
                        match file_storage::get_file_as_string(&bucket, &path).await {
                            Ok(Some(content)) => {
                                scripts.push((name, content));
                            }
                            Ok(None) => {
                                log::warn!("Custom script file not found: {}", path);
                            }
                            Err(e) => {
                                log::warn!("Error reading custom script {}: {e}", path);
                            }
                        }
                    }
                    let _ = tx.send(scripts);
                }
                Err(e) => {
                    log::warn!("Error listing custom scripts: {e}");
                    let _ = tx.send(Vec::new());
                }
            }
        });
    }

    fn poll_custom_scripts(&mut self) {
        if let Ok(scripts) = self.custom_scripts_rx.try_recv() {
            self.custom_scripts_loaded = true;
            if scripts.is_empty() {
                return;
            }
            let items: Vec<ScriptCheckItem> = scripts
                .into_iter()
                .map(|(name, content)| ScriptCheckItem {
                    category: "Custom Scripts".into(),
                    name,
                    checked: false,
                    status: None,
                    content: Some(content),
                })
                .collect();
            self.categories.push(ScriptCategoryGroup {
                name: "Custom Scripts".into(),
                items,
            });
        }
    }

    pub fn append_log(&mut self, msg: String) {
        if msg.contains(crate::scripts::REBOOT_RECOMMENDED_MARKER) {
            self.reboot_recommended = true;
        }
        self.log_messages.push(msg);
        // A shorter buffer makes `HomePage::ingest_script_log` re-parse from the start.
        if self.log_messages.len() > MAX_LOG_LINES {
            let excess = self.log_messages.len() - MAX_LOG_LINES;
            self.log_messages.drain(..excess);
        }
    }

    pub fn set_script_result(&mut self, name: String, status: RemoteScriptStatus) {
        for cat in &mut self.categories {
            if let Some(item) = cat.items.iter_mut().find(|i| i.name == name) {
                item.status = Some(status);
                return;
            }
        }
    }

    pub fn set_complete(&mut self) {
        self.running = false;
        self.log_messages
            .push("--- All scripts completed ---".into());
    }

    fn get_selected_scripts(&self) -> Vec<RemoteScriptItem> {
        self.categories
            .iter()
            .flat_map(|cat| {
                cat.items
                    .iter()
                    .filter(|i| i.checked)
                    .map(|i| RemoteScriptItem {
                        name: i.name.clone(),
                        category: i.category.clone(),
                        content: i.content.clone(),
                    })
            })
            .collect()
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        self.poll_custom_scripts();

        if self.loading {
            controls::loading_state(ui, "Loading scripts…");
            return;
        }

        if self.categories.is_empty() {
            controls::empty_state(
                ui,
                svg::SCROLL,
                "No script list received",
                "Click the Scripts tab to request it.",
                &[],
            );
            return;
        }

        let panel_id = ui.id().with("scripts_top_bar");
        eframe::egui::Panel::top(panel_id).show(ui, |ui| {
            controls::bar(ui, BarKind::Tool, |ui| {
                TextField::new(&mut self.service_number)
                    .hint("SO #")
                    .icon(svg::RECEIPT)
                    .width(120.0)
                    .enabled(!self.running)
                    .show(ui);
                TextField::new(&mut self.customer_email)
                    .hint("Customer email")
                    .icon(svg::ENVELOPE_SIMPLE)
                    .width(200.0)
                    .enabled(!self.running)
                    .show(ui);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.running {
                        controls::spinner(ui, SpinnerSize::Sm);
                        ui.label("Running...");
                    } else {
                        let run = TextButton::new("Run Selected")
                            .icon(svg::PLAY)
                            .kind(ButtonKind::Primary)
                            .show(ui);
                        if run.clicked() {
                            let scripts = self.get_selected_scripts();
                            if !scripts.is_empty() {
                                self.running = true;
                                self.reboot_recommended = false;
                                self.log_messages.clear();
                                for cat in &mut self.categories {
                                    for item in &mut cat.items {
                                        if item.checked {
                                            item.status = Some(RemoteScriptStatus::Running);
                                        }
                                    }
                                }
                                let _ = cmd_tx.try_send(Cmd::RunRemoteScripts {
                                    scripts,
                                    service_number: self.service_number.clone(),
                                    customer_email: self.customer_email.clone(),
                                    diagnostic_session_id: String::new(),
                                });
                            }
                        }
                        let select_all =
                            controls::icon_text_button(ui, svg::CHECK_SQUARE, "Select All");
                        if select_all.clicked()
                            && let Some(cat) = self.categories.get_mut(self.selected_category)
                        {
                            let all_checked = cat.items.iter().all(|i| i.checked);
                            for item in &mut cat.items {
                                item.checked = !all_checked;
                            }
                        }
                        if controls::icon_text_button(ui, svg::ERASER, "Clear Log").clicked() {
                            self.log_messages.clear();
                        }
                    }
                });
            });
        });

        if self.reboot_recommended && !self.running {
            let banner_id = ui.id().with("scripts_reboot_banner");
            eframe::egui::Panel::top(banner_id).show(ui, |ui| {
                controls::bar(ui, BarKind::Status, |ui| {
                    controls::note(
                        ui,
                        svg::WARNING,
                        "Webroot was re-keyed — a client reboot finalizes the new device \
                         identity.",
                        theme::warn(ui),
                    );
                    let reboot = controls::icon_text_button(
                        ui,
                        svg::ARROW_CLOCKWISE,
                        "Reboot client & relaunch MasterTech",
                    );
                    if reboot.clicked() {
                        let _ = cmd_tx.try_send(Cmd::RebootSystem {
                            persist_mastertech: true,
                            terminal_mode: false,
                        });
                        self.log_messages
                            .push("Reboot sent — client relaunches MasterTech after login.".into());
                        self.reboot_recommended = false;
                    }
                    if controls::ghost_button(ui, "Dismiss").clicked() {
                        self.reboot_recommended = false;
                    }
                });
            });
        }

        let side_id = ui.id().with("scripts_side_panel");
        eframe::egui::Panel::left(side_id)
            .resizable(false)
            .default_size(160.0_f32)
            .show(ui, |ui| {
                glass_card::card_header(ui, svg::FOLDERS, "Categories", None);
                for (i, cat) in self.categories.iter().enumerate() {
                    let selected = i == self.selected_category;
                    let checked_count = cat.items.iter().filter(|it| it.checked).count();
                    let label = if checked_count > 0 {
                        format!("{} ({})", cat.name, checked_count)
                    } else {
                        cat.name.clone()
                    };
                    let icon = if selected {
                        svg::FOLDER_OPEN
                    } else {
                        svg::FOLDER
                    };
                    let row = ListRow::new(&label)
                        .lead(Lead::Icon(icon, None))
                        .selected(selected)
                        .show(ui);
                    if row.clicked() {
                        self.selected_category = i;
                    }
                }
            });

        let log_panel_id = ui.id().with("scripts_log_panel");
        eframe::egui::Panel::bottom(log_panel_id)
            .resizable(true)
            .default_size(200.0_f32)
            .min_size(80.0_f32)
            .show(ui, |ui| {
                let lines = format!("{} lines", self.log_messages.len());
                glass_card::card_header(ui, svg::TERMINAL_WINDOW, "Script Log", Some(&lines));
                let scroll = ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.auto_scroll);
                scroll.show(ui, |ui| {
                    for msg in &self.log_messages {
                        let color = if msg.starts_with("Error")
                            || msg.contains("error")
                            || msg.contains("Failed")
                        {
                            theme::error(ui)
                        } else if msg.starts_with("Starting:") || msg.starts_with("---") {
                            theme::info(ui)
                        } else {
                            theme::text(ui)
                        };
                        ui.add(Label::new(RichText::new(msg).color(color).monospace()));
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(cat) = self.categories.get_mut(self.selected_category) {
                glass_card::card_header(ui, svg::FOLDER_OPEN, &cat.name, None);

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in &mut cat.items {
                            ui.horizontal(|ui| {
                                let status_icon = match &item.status {
                                    Some(RemoteScriptStatus::Running) => {
                                        controls::spinner(ui, SpinnerSize::Sm);
                                        None
                                    }
                                    Some(RemoteScriptStatus::Success) => {
                                        Some((svg::CHECK_CIRCLE, theme::success(ui)))
                                    }
                                    Some(RemoteScriptStatus::Failed) => {
                                        Some((svg::X_CIRCLE, theme::error(ui)))
                                    }
                                    None => None,
                                };

                                controls::checkbox(ui, &mut item.checked, &item.name);

                                if let Some((icon, tone)) = status_icon {
                                    icon.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, tone);
                                }

                                if item.content.is_some() {
                                    svg::FILE_CODE
                                        .show_tinted(
                                            ui,
                                            Metrics::get(ui.ctx()).icon_sm,
                                            theme::weak_text(ui),
                                        )
                                        .on_hover_text("Custom script");
                                }
                            });
                        }
                    });
            }
        });
    }
}

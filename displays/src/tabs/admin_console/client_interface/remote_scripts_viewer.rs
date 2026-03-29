use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{
    self, Align, Color32, Label, Layout, RichText, ScrollArea, TextEdit,
    Ui,
};

use crate::{Cmd, PlatformSpawner, RemoteScriptItem, RemoteScriptStatus, Spawner};

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
    log_messages: Vec<String>,
    pub running: bool,
    pub loading: bool,
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
        self.log_messages.push(msg);
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
        self.log_messages.push("--- All scripts completed ---".into());
    }

    fn get_selected_scripts(&self) -> Vec<RemoteScriptItem> {
        self.categories
            .iter()
            .flat_map(|cat| {
                cat.items.iter().filter(|i| i.checked).map(|i| RemoteScriptItem {
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
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
            return;
        }

        if self.categories.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No script list received. Click the Scripts tab to request it.");
            });
            return;
        }

        let panel_id = ui.id().with("scripts_top_bar");
        eframe::egui::Panel::top(panel_id).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("SO #:");
                ui.add(
                    TextEdit::singleline(&mut self.service_number)
                        .desired_width(120.0)
                        .interactive(!self.running),
                );
                ui.label("Email:");
                ui.add(
                    TextEdit::singleline(&mut self.customer_email)
                        .desired_width(180.0)
                        .interactive(!self.running),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.running {
                        ui.spinner();
                        ui.label("Running...");
                    } else {
                        if ui.button("Run Selected").clicked() {
                            let scripts = self.get_selected_scripts();
                            if !scripts.is_empty() {
                                self.running = true;
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
                                });
                            }
                        }
                        if ui.button("Select All").clicked() {
                            if let Some(cat) = self.categories.get_mut(self.selected_category) {
                                let all_checked = cat.items.iter().all(|i| i.checked);
                                for item in &mut cat.items {
                                    item.checked = !all_checked;
                                }
                            }
                        }
                        if ui.button("Clear Log").clicked() {
                            self.log_messages.clear();
                        }
                    }
                });
            });
        });

        let side_id = ui.id().with("scripts_side_panel");
        eframe::egui::Panel::left(side_id)
            .resizable(false)
            .default_size(160.0)
            .show_inside(ui, |ui| {
                ui.heading("Categories");
                ui.separator();
                for (i, cat) in self.categories.iter().enumerate() {
                    let selected = i == self.selected_category;
                    let checked_count = cat.items.iter().filter(|it| it.checked).count();
                    let label = if checked_count > 0 {
                        format!("{} ({})", cat.name, checked_count)
                    } else {
                        cat.name.clone()
                    };
                    if ui.selectable_label(selected, &label).clicked() {
                        self.selected_category = i;
                    }
                }
            });

        let log_panel_id = ui.id().with("scripts_log_panel");
        eframe::egui::Panel::bottom(log_panel_id)
            .resizable(true)
            .default_height(200.0)
            .min_height(80.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Script Log");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("{} lines", self.log_messages.len()));
                    });
                });
                ui.separator();
                let scroll = ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.auto_scroll);
                scroll.show(ui, |ui| {
                    for msg in &self.log_messages {
                        let color = if msg.starts_with("Error") || msg.contains("error") || msg.contains("Failed") {
                            Color32::from_rgb(255, 100, 100)
                        } else if msg.starts_with("Starting:") || msg.starts_with("---") {
                            Color32::from_rgb(140, 180, 255)
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.add(Label::new(RichText::new(msg).color(color).monospace()));
                    }
                });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(cat) = self.categories.get_mut(self.selected_category) {
                ui.heading(&cat.name);
                ui.separator();

                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for item in &mut cat.items {
                        ui.horizontal(|ui| {
                            let status_icon = match &item.status {
                                Some(RemoteScriptStatus::Running) => {
                                    ui.spinner();
                                    None
                                }
                                Some(RemoteScriptStatus::Success) => {
                                    Some(RichText::new("✓").color(Color32::from_rgb(80, 200, 80)).strong())
                                }
                                Some(RemoteScriptStatus::Failed) => {
                                    Some(RichText::new("✗").color(Color32::from_rgb(255, 80, 80)).strong())
                                }
                                None => None,
                            };

                            ui.checkbox(&mut item.checked, "");

                            if let Some(icon) = status_icon {
                                ui.label(icon);
                            }

                            if item.content.is_some() {
                                ui.label(RichText::new(&item.name).italics());
                            } else {
                                ui.label(&item.name);
                            }
                        });
                    }
                });
            }
        });
    }
}

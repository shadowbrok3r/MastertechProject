use eframe::egui::{
    self, Color32, KeyboardShortcut, RichText, ScrollArea, TextEdit, Ui,
    Widget, scroll_area,
};
use crossbeam::channel::{Sender, Receiver};
use egui_data_table::{
    viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext, CustomActionContext, CustomActionEditor},
    CustomMenuItem, DataTable, Renderer, RowViewer, SelectionSnapshot, UiAction,
};
use egui_extras::Column as TableColumnConfig;
use serde::Serialize;
use crate::{Cmd, ServiceActionType, WindowsService};

const NUM_COLUMNS: usize = 5;

#[derive(Debug, Clone)]
pub enum ServiceViewerAction {
    Start(String),
    Stop(String),
    Restart(String),
    SetStartType(String, String),
    Refresh,
}

#[derive(Serialize)]
pub struct ServiceRowViewer {
    pub filter: String,
    pub running_only: bool,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<ServiceViewerAction>>,
}

impl Default for ServiceRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            running_only: false,
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

pub struct ServiceCodec;

impl RowCodec<WindowsService> for ServiceCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &WindowsService, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&row.status),
            1 => dst.push_str(&row.name),
            2 => dst.push_str(&row.display_name),
            3 => dst.push_str(&row.start_type),
            4 => {
                if let Some(pid) = row.pid {
                    dst.push_str(&pid.to_string());
                }
            }
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src: &str,
        column: usize,
        row: &mut WindowsService,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => row.status = src.to_string(),
            1 => row.name = src.to_string(),
            2 => row.display_name = src.to_string(),
            3 => row.start_type = src.to_string(),
            4 => row.pid = src.parse().ok(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> WindowsService {
        WindowsService {
            name: String::new(),
            display_name: String::new(),
            status: String::new(),
            start_type: String::new(),
            pid: None,
        }
    }
}

pub struct ServicesViewer {
    pub table: DataTable<WindowsService>,
    pub viewer: ServiceRowViewer,
    pub action_rx: Receiver<ServiceViewerAction>,
    action_tx: Sender<ServiceViewerAction>,
    pub loading: bool,
    pub entries: Vec<WindowsService>,
    pub status_message: Option<(String, bool)>,
    pub confirm_action: Option<(String, ServiceViewerAction)>,
}

impl Default for ServicesViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicesViewer {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut viewer = ServiceRowViewer::default();
        viewer.action_tx = Some(action_tx.clone());

        Self {
            table: DataTable::new(),
            viewer,
            action_rx,
            action_tx,
            loading: false,
            entries: Vec::new(),
            status_message: None,
            confirm_action: None,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<WindowsService>) {
        self.entries = entries.clone();
        self.table.replace(entries);
        self.loading = false;
    }

    pub fn set_action_result(&mut self, name: String, success: bool, message: String) {
        self.status_message = Some((format!("{}: {}", name, message), success));
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        while let Ok(action) = self.action_rx.try_recv() {
            match &action {
                ServiceViewerAction::Start(name) => {
                    let _ = cmd_tx.try_send(Cmd::ControlService {
                        name: name.clone(),
                        action: ServiceActionType::Start,
                    });
                }
                ServiceViewerAction::Stop(name) => {
                    self.confirm_action = Some((
                        format!("Are you sure you want to stop '{}'?", name),
                        action.clone(),
                    ));
                }
                ServiceViewerAction::Restart(name) => {
                    self.confirm_action = Some((
                        format!("Are you sure you want to restart '{}'?", name),
                        action.clone(),
                    ));
                }
                ServiceViewerAction::SetStartType(name, start_type) => {
                    let _ = cmd_tx.try_send(Cmd::ControlService {
                        name: name.clone(),
                        action: ServiceActionType::SetStartType(start_type.clone()),
                    });
                }
                ServiceViewerAction::Refresh => {
                    self.loading = true;
                    self.table.clear();
                    self.entries.clear();
                    let _ = cmd_tx.try_send(Cmd::ListServices);
                }
            }
        }

        // Confirmation dialog
        if let Some((msg, pending_action)) = self.confirm_action.clone() {
            egui::Window::new("Confirm Action")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(&msg);
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            match &pending_action {
                                ServiceViewerAction::Stop(name) => {
                                    let _ = cmd_tx.try_send(Cmd::ControlService {
                                        name: name.clone(),
                                        action: ServiceActionType::Stop,
                                    });
                                }
                                ServiceViewerAction::Restart(name) => {
                                    let _ = cmd_tx.try_send(Cmd::ControlService {
                                        name: name.clone(),
                                        action: ServiceActionType::Restart,
                                    });
                                }
                                _ => {}
                            }
                            self.confirm_action = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_action = None;
                        }
                    });
                });
        }

        // Toolbar
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.loading = true;
                self.table.clear();
                self.entries.clear();
                let _ = cmd_tx.try_send(Cmd::ListServices);
            }

            ui.label("Filter:");
            TextEdit::singleline(&mut self.viewer.filter)
                .hint_text("Search services...")
                .desired_width(200.)
                .show(ui);

            ui.checkbox(&mut self.viewer.running_only, "Running only");

            if let Some((msg, success)) = &self.status_message {
                let color = if *success { Color32::GREEN } else { Color32::RED };
                ui.colored_label(color, msg);
            }
        });

        ui.add_space(4.);

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading services...");
            });
            return;
        }

        if self.entries.is_empty() {
            ui.label(RichText::new("No services loaded. Click Refresh to fetch.").italics().color(Color32::GRAY));
            return;
        }

        ScrollArea::horizontal()
            .auto_shrink(false)
            .show(ui, |ui| {
                Renderer::new(&mut self.table, &mut self.viewer)
                    .with_style_modify(|s| {
                        s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                        s.auto_shrink = [false, false].into();
                    })
                    .ui(ui)
            });
    }
}

impl RowViewer<WindowsService> for ServiceRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<WindowsService>> {
        Some(ServiceCodec)
    }

    fn num_columns(&mut self) -> usize { NUM_COLUMNS }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Status", "Name", "Display Name", "Start Type", "PID"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &WindowsService) -> bool {
        if self.running_only && row.status != "Running" {
            return false;
        }
        if self.filter.trim().is_empty() { return true; }
        let f = self.filter.to_lowercase();
        row.name.to_lowercase().contains(&f)
            || row.display_name.to_lowercase().contains(&f)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &WindowsService) -> bool { false }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &WindowsService, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let (icon, color) = match row.status.as_str() {
                    "Running" => ("▶", Color32::GREEN),
                    "Stopped" => ("■", Color32::from_rgb(180, 80, 80)),
                    "Paused" => ("⏸", Color32::YELLOW),
                    "StartPending" => ("⏳", Color32::YELLOW),
                    "StopPending" => ("⏳", Color32::from_rgb(255, 150, 50)),
                    _ => ("●", Color32::GRAY),
                };
                ui.label(RichText::new(format!("{} {}", icon, row.status)).color(color));
            }
            1 => {
                ui.label(RichText::new(&row.name).color(Color32::from_rgb(180, 200, 220)));
            }
            2 => {
                ui.label(RichText::new(&row.display_name).color(Color32::from_rgb(220, 220, 220)));
            }
            3 => {
                let color = match row.start_type.as_str() {
                    "Automatic" | "Auto" => Color32::GREEN,
                    "Manual" => Color32::YELLOW,
                    "Disabled" => Color32::from_rgb(180, 80, 80),
                    _ => Color32::GRAY,
                };
                ui.label(RichText::new(&row.start_type).color(color));
            }
            4 => {
                let text = row.pid.map(|p| p.to_string()).unwrap_or_default();
                ui.label(RichText::new(text).color(Color32::GRAY));
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut WindowsService,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        _row: &WindowsService,
        _column: usize,
        _resp: &egui::Response,
    ) -> Option<Box<WindowsService>> {
        None
    }

    fn set_cell_value(&mut self, src: &WindowsService, dst: &mut WindowsService, column: usize) {
        match column {
            0 => dst.status = src.status.clone(),
            1 => dst.name = src.name.clone(),
            2 => dst.display_name = src.display_name.clone(),
            3 => dst.start_type = src.start_type.clone(),
            4 => dst.pid = src.pid,
            _ => {}
        }
    }

    fn compare_cell(&self, l: &WindowsService, r: &WindowsService, column: usize) -> std::cmp::Ordering {
        match column {
            0 => status_order(&l.status).cmp(&status_order(&r.status)),
            1 => l.name.to_lowercase().cmp(&r.name.to_lowercase()),
            2 => l.display_name.to_lowercase().cmp(&r.display_name.to_lowercase()),
            3 => l.start_type.cmp(&r.start_type),
            4 => l.pid.unwrap_or(0).cmp(&r.pid.unwrap_or(0)),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> WindowsService {
        WindowsService {
            name: String::new(),
            display_name: String::new(),
            status: String::new(),
            start_type: String::new(),
            pid: None,
        }
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(90.).at_most(130.),
            1 => base.at_least(180.).clip(true).resizable(true),
            2 => base.at_least(250.).clip(true).resizable(true),
            3 => base.at_least(90.).at_most(120.),
            4 => base.at_least(60.).at_most(90.),
            _ => base,
        }
    }

    fn custom_context_menu_items(
        &mut self,
        _context: &UiActionContext,
        selection: &SelectionSnapshot<'_, WindowsService>,
    ) -> Vec<CustomMenuItem> {
        let has_selection = !selection.selected_rows.is_empty();
        let first = selection.selected_rows.first().map(|(_, r)| r);
        let is_running = first.map(|r| r.status == "Running").unwrap_or(false);
        let is_stopped = first.map(|r| r.status == "Stopped").unwrap_or(false);

        let mut items = Vec::new();
        items.push(CustomMenuItem::new("start", "Start").icon("▶").enabled(has_selection && is_stopped));
        items.push(CustomMenuItem::new("stop", "Stop").icon("■").enabled(has_selection && is_running));
        items.push(CustomMenuItem::new("restart", "Restart").icon("🔄").enabled(has_selection && is_running));
        items.push(CustomMenuItem::new("set_auto", "Set Automatic").icon("⚡").enabled(has_selection));
        items.push(CustomMenuItem::new("set_manual", "Set Manual").icon("🔧").enabled(has_selection));
        items.push(CustomMenuItem::new("set_disabled", "Set Disabled").icon("🚫").enabled(has_selection));
        items.push(CustomMenuItem::new("refresh", "Refresh").icon("⟲").enabled(true));
        items
    }

    fn on_custom_action_ex(
        &mut self,
        action_id: &'static str,
        ctx: &CustomActionContext<'_, WindowsService>,
        _editor: &mut CustomActionEditor<WindowsService>,
    ) {
        let Some(tx) = &self.action_tx else { return };
        let first = ctx.selection.selected_rows.first().map(|(_, r)| r);

        match action_id {
            "start" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ServiceViewerAction::Start(row.name.clone()));
                }
            }
            "stop" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ServiceViewerAction::Stop(row.name.clone()));
                }
            }
            "restart" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ServiceViewerAction::Restart(row.name.clone()));
                }
            }
            "set_auto" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ServiceViewerAction::SetStartType(row.name.clone(), "Automatic".to_string()));
                }
            }
            "set_manual" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ServiceViewerAction::SetStartType(row.name.clone(), "Manual".to_string()));
                }
            }
            "set_disabled" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ServiceViewerAction::SetStartType(row.name.clone(), "Disabled".to_string()));
                }
            }
            "refresh" => {
                let _ = tx.try_send(ServiceViewerAction::Refresh);
            }
            _ => {}
        }
    }
}

fn status_order(status: &str) -> u8 {
    match status {
        "Running" => 0,
        "StartPending" => 1,
        "Paused" => 2,
        "StopPending" => 3,
        "Stopped" => 4,
        _ => 5,
    }
}

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
use crate::{Cmd, StartupApp};
use crate::ui_tools::icons;

const NUM_COLUMNS: usize = 5;

#[derive(Debug, Clone)]
pub enum StartupAction {
    Enable(String, String),
    Disable(String, String),
    Refresh,
}

#[derive(Serialize)]
pub struct StartupAppRowViewer {
    pub filter: String,
    pub show_mode: ShowMode,
    filter_key: String,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<StartupAction>>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShowMode {
    All,
    EnabledOnly,
    DisabledOnly,
}

impl Default for StartupAppRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            show_mode: ShowMode::All,
            filter_key: String::new(),
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

pub struct StartupAppCodec;

impl RowCodec<StartupApp> for StartupAppCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &StartupApp, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&row.state),
            1 => dst.push_str(&row.name),
            2 => dst.push_str(&row.command),
            3 => dst.push_str(&row.source),
            4 => dst.push_str(&row.registry_path),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src: &str,
        column: usize,
        row: &mut StartupApp,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => row.state = src.to_string(),
            1 => row.name = src.to_string(),
            2 => row.command = src.to_string(),
            3 => row.source = src.to_string(),
            4 => row.registry_path = src.to_string(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> StartupApp {
        StartupApp {
            name: String::new(),
            command: String::new(),
            registry_path: String::new(),
            state: String::new(),
            source: String::new(),
        }
    }
}

pub struct StartupAppsViewer {
    pub table: DataTable<StartupApp>,
    pub viewer: StartupAppRowViewer,
    pub action_rx: Receiver<StartupAction>,
    pub loading: bool,
    pub entries: Vec<StartupApp>,
    pub status_message: Option<(String, bool)>,
}

impl Default for StartupAppsViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl StartupAppsViewer {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut viewer = StartupAppRowViewer::default();
        viewer.action_tx = Some(action_tx);

        Self {
            table: DataTable::new(),
            viewer,
            action_rx,
            loading: false,
            entries: Vec::new(),
            status_message: None,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<StartupApp>) {
        self.entries = entries.clone();
        self.table.replace(entries);
        self.loading = false;
    }

    pub fn set_action_result(&mut self, success: bool, message: String) {
        self.status_message = Some((message, success));
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                StartupAction::Enable(name, path) => {
                    let _ = cmd_tx.try_send(Cmd::ToggleStartupApp {
                        name,
                        registry_path: path,
                        enable: true,
                    });
                }
                StartupAction::Disable(name, path) => {
                    let _ = cmd_tx.try_send(Cmd::ToggleStartupApp {
                        name,
                        registry_path: path,
                        enable: false,
                    });
                }
                StartupAction::Refresh => {
                    self.loading = true;
                    self.table.clear();
                    self.entries.clear();
                    let _ = cmd_tx.try_send(Cmd::ListStartupApps);
                }
            }
        }

        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.loading = true;
                self.table.clear();
                self.entries.clear();
                let _ = cmd_tx.try_send(Cmd::ListStartupApps);
            }

            ui.label("Filter:");
            TextEdit::singleline(&mut self.viewer.filter)
                .hint_text("Search startup apps...")
                .desired_width(200.)
                .show(ui);

            ui.separator();
            ui.selectable_value(&mut self.viewer.show_mode, ShowMode::All, "All");
            ui.selectable_value(&mut self.viewer.show_mode, ShowMode::EnabledOnly, "Enabled");
            ui.selectable_value(&mut self.viewer.show_mode, ShowMode::DisabledOnly, "Disabled");

            if let Some((msg, success)) = &self.status_message {
                let color = if *success { Color32::GREEN } else { Color32::RED };
                ui.colored_label(color, msg);
            }
        });

        ui.add_space(4.);

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading startup apps...");
            });
            return;
        }

        if self.entries.is_empty() {
            ui.label(RichText::new("No startup apps loaded. Click Refresh to fetch.").italics().color(Color32::GRAY));
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

impl RowViewer<StartupApp> for StartupAppRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<StartupApp>> {
        Some(StartupAppCodec)
    }

    fn num_columns(&mut self) -> usize { NUM_COLUMNS }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["State", "Name", "Command", "Source", "Registry Path"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        let mode_tag = match self.show_mode {
            ShowMode::All => "a",
            ShowMode::EnabledOnly => "e",
            ShowMode::DisabledOnly => "d",
        };
        self.filter_key = format!("{}|{}", self.filter, mode_tag);
        &self.filter_key
    }

    fn filter_row(&mut self, row: &StartupApp) -> bool {
        match self.show_mode {
            ShowMode::EnabledOnly if row.state != "Enabled" => return false,
            ShowMode::DisabledOnly if !row.state.contains("Disabled") => return false,
            _ => {}
        }
        if self.filter.trim().is_empty() { return true; }
        let f = self.filter.to_lowercase();
        row.name.to_lowercase().contains(&f)
            || row.command.to_lowercase().contains(&f)
            || row.source.to_lowercase().contains(&f)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &StartupApp) -> bool { false }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &StartupApp, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let (icon, color) = match row.state.as_str() {
                    "Enabled" => ("●", Color32::GREEN),
                    "Disabled" | "DisabledByUser" => ("○", Color32::from_rgb(180, 80, 80)),
                    _ => ("?", Color32::GRAY),
                };
                ui.label(RichText::new(format!("{icon} {}", row.state)).color(color));
            }
            1 => {
                ui.label(RichText::new(&row.name).color(Color32::from_rgb(200, 210, 230)));
            }
            2 => {
                ui.label(RichText::new(&row.command).color(Color32::from_rgb(180, 180, 180)));
            }
            3 => {
                let color = if row.source.contains("HKLM") {
                    Color32::from_rgb(200, 170, 120)
                } else {
                    Color32::from_rgb(120, 170, 200)
                };
                ui.label(RichText::new(&row.source).color(color));
            }
            4 => {
                ui.label(RichText::new(&row.registry_path).color(Color32::GRAY));
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut StartupApp,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        _row: &StartupApp,
        _column: usize,
        _resp: &egui::Response,
    ) -> Option<Box<StartupApp>> {
        None
    }

    fn set_cell_value(&mut self, src: &StartupApp, dst: &mut StartupApp, column: usize) {
        match column {
            0 => dst.state = src.state.clone(),
            1 => dst.name = src.name.clone(),
            2 => dst.command = src.command.clone(),
            3 => dst.source = src.source.clone(),
            4 => dst.registry_path = src.registry_path.clone(),
            _ => {}
        }
    }

    fn compare_cell(&self, l: &StartupApp, r: &StartupApp, column: usize) -> std::cmp::Ordering {
        match column {
            0 => state_order(&l.state).cmp(&state_order(&r.state)),
            1 => l.name.to_lowercase().cmp(&r.name.to_lowercase()),
            2 => l.command.to_lowercase().cmp(&r.command.to_lowercase()),
            3 => l.source.cmp(&r.source),
            4 => l.registry_path.cmp(&r.registry_path),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> StartupApp {
        StartupApp {
            name: String::new(),
            command: String::new(),
            registry_path: String::new(),
            state: String::new(),
            source: String::new(),
        }
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(90.).at_most(140.),
            1 => base.at_least(180.).clip(true).resizable(true),
            2 => base.at_least(280.).clip(true).resizable(true),
            3 => base.at_least(100.).at_most(160.).clip(true),
            4 => base.at_least(250.).clip(true).resizable(true),
            _ => base,
        }
    }

    fn custom_context_menu_items(
        &mut self,
        _context: &UiActionContext,
        selection: &SelectionSnapshot<'_, StartupApp>,
    ) -> Vec<CustomMenuItem> {
        let has_selection = !selection.selected_rows.is_empty();
        let first = selection.selected_rows.first().map(|(_, r)| r);
        let is_enabled = first.map(|r| r.state == "Enabled").unwrap_or(false);
        let is_disabled = first.map(|r| r.state.contains("Disabled")).unwrap_or(false);

        let mut items = Vec::new();
        items.push(CustomMenuItem::new("enable", "Enable").icon(icons::STATUS_ON).enabled(has_selection && is_disabled));
        items.push(CustomMenuItem::new("disable", "Disable").icon(icons::STATUS_IDLE).enabled(has_selection && is_enabled));
        items.push(CustomMenuItem::new("refresh", "Refresh").icon(icons::REFRESH).enabled(true));
        items
    }

    fn on_custom_action_ex(
        &mut self,
        action_id: &'static str,
        ctx: &CustomActionContext<'_, StartupApp>,
        _editor: &mut CustomActionEditor<StartupApp>,
    ) {
        let Some(tx) = &self.action_tx else { return };
        let first = ctx.selection.selected_rows.first().map(|(_, r)| r);

        match action_id {
            "enable" => {
                if let Some(row) = first {
                    let _ = tx.try_send(StartupAction::Enable(row.name.clone(), row.registry_path.clone()));
                }
            }
            "disable" => {
                if let Some(row) = first {
                    let _ = tx.try_send(StartupAction::Disable(row.name.clone(), row.registry_path.clone()));
                }
            }
            "refresh" => {
                let _ = tx.try_send(StartupAction::Refresh);
            }
            _ => {}
        }
    }
}

fn state_order(state: &str) -> u8 {
    match state {
        "Enabled" => 0,
        "Disabled" => 1,
        "DisabledByUser" => 2,
        _ => 3,
    }
}

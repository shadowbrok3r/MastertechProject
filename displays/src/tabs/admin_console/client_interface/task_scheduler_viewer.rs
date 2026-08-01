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
use crate::{Cmd, ScheduledTask};
use crate::ui_tools::icons;

const NUM_COLUMNS: usize = 7;

#[derive(Debug, Clone)]
pub enum TaskSchedulerAction {
    Enable(String),
    Disable(String),
    RunNow(String),
    ShowDetail(ScheduledTask),
    Refresh,
}

#[derive(Serialize)]
pub struct TaskSchedulerRowViewer {
    pub filter: String,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<TaskSchedulerAction>>,
}

impl Default for TaskSchedulerRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

pub struct TaskSchedulerCodec;

impl RowCodec<ScheduledTask> for TaskSchedulerCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &ScheduledTask, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&row.state),
            1 => dst.push_str(&row.name),
            2 => dst.push_str(&row.path),
            3 => dst.push_str(row.last_run.as_deref().unwrap_or("")),
            4 => dst.push_str(row.next_run.as_deref().unwrap_or("")),
            5 => {
                let summary = row.triggers.join("; ");
                dst.push_str(&summary);
            }
            6 => {
                let summary = row.actions.join("; ");
                dst.push_str(&summary);
            }
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src: &str,
        column: usize,
        row: &mut ScheduledTask,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => row.state = src.to_string(),
            1 => row.name = src.to_string(),
            2 => row.path = src.to_string(),
            3 => row.last_run = if src.is_empty() { None } else { Some(src.to_string()) },
            4 => row.next_run = if src.is_empty() { None } else { Some(src.to_string()) },
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> ScheduledTask {
        ScheduledTask {
            name: String::new(),
            path: String::new(),
            state: String::new(),
            last_run: None,
            next_run: None,
            description: String::new(),
            triggers: Vec::new(),
            actions: Vec::new(),
        }
    }
}

pub struct TaskSchedulerViewer {
    pub table: DataTable<ScheduledTask>,
    pub viewer: TaskSchedulerRowViewer,
    pub action_rx: Receiver<TaskSchedulerAction>,
    pub loading: bool,
    pub entries: Vec<ScheduledTask>,
    pub detail_task: Option<ScheduledTask>,
    pub status_message: Option<(String, bool)>,
}

impl Default for TaskSchedulerViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskSchedulerViewer {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut viewer = TaskSchedulerRowViewer::default();
        viewer.action_tx = Some(action_tx);

        Self {
            table: DataTable::new(),
            viewer,
            action_rx,
            loading: false,
            entries: Vec::new(),
            detail_task: None,
            status_message: None,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<ScheduledTask>) {
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
                TaskSchedulerAction::Enable(path) => {
                    let _ = cmd_tx.try_send(Cmd::ToggleScheduledTask { path, enable: true });
                }
                TaskSchedulerAction::Disable(path) => {
                    let _ = cmd_tx.try_send(Cmd::ToggleScheduledTask { path, enable: false });
                }
                TaskSchedulerAction::RunNow(path) => {
                    let _ = cmd_tx.try_send(Cmd::RunScheduledTask(path));
                }
                TaskSchedulerAction::ShowDetail(task) => {
                    self.detail_task = Some(task);
                }
                TaskSchedulerAction::Refresh => {
                    self.loading = true;
                    self.table.clear();
                    self.entries.clear();
                    let _ = cmd_tx.try_send(Cmd::ListScheduledTasks { folder: None });
                }
            }
        }

        // Toolbar
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.loading = true;
                self.table.clear();
                self.entries.clear();
                let _ = cmd_tx.try_send(Cmd::ListScheduledTasks { folder: None });
            }

            ui.label("Filter:");
            TextEdit::singleline(&mut self.viewer.filter)
                .hint_text("Search tasks...")
                .desired_width(200.)
                .show(ui);

            if let Some((msg, success)) = &self.status_message {
                let color = if *success { Color32::GREEN } else { Color32::RED };
                ui.colored_label(color, msg);
            }
        });

        ui.add_space(4.);

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading scheduled tasks...");
            });
            return;
        }

        if self.entries.is_empty() {
            ui.label(RichText::new("No tasks loaded. Click Refresh to fetch.").italics().color(Color32::GRAY));
            return;
        }

        // Detail window
        if let Some(task) = self.detail_task.clone() {
            egui::Window::new(format!("Task: {}", task.name))
                .collapsible(true)
                .resizable(true)
                .default_width(500.)
                .show(ui.ctx(), |ui| {
                    egui::Grid::new("task_detail_grid").num_columns(2).show(ui, |ui| {
                        ui.label(RichText::new("Name:").strong());
                        ui.label(&task.name);
                        ui.end_row();

                        ui.label(RichText::new("Path:").strong());
                        ui.label(&task.path);
                        ui.end_row();

                        ui.label(RichText::new("State:").strong());
                        let color = state_color(&task.state);
                        ui.colored_label(color, &task.state);
                        ui.end_row();

                        ui.label(RichText::new("Last Run:").strong());
                        ui.label(task.last_run.as_deref().unwrap_or("Never"));
                        ui.end_row();

                        ui.label(RichText::new("Next Run:").strong());
                        ui.label(task.next_run.as_deref().unwrap_or("N/A"));
                        ui.end_row();

                        ui.label(RichText::new("Description:").strong());
                        ui.label(&task.description);
                        ui.end_row();
                    });

                    ui.add_space(8.);
                    ui.label(RichText::new("Triggers:").strong());
                    for trigger in &task.triggers {
                        ui.label(format!("  - {}", trigger));
                    }

                    ui.add_space(4.);
                    ui.label(RichText::new("Actions:").strong());
                    for action in &task.actions {
                        ui.label(format!("  - {}", action));
                    }

                    ui.add_space(8.);
                    if ui.button("Close").clicked() {
                        self.detail_task = None;
                    }
                });
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

impl RowViewer<ScheduledTask> for TaskSchedulerRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<ScheduledTask>> {
        Some(TaskSchedulerCodec)
    }

    fn num_columns(&mut self) -> usize { NUM_COLUMNS }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["State", "Name", "Path", "Last Run", "Next Run", "Triggers", "Actions"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &ScheduledTask) -> bool {
        if self.filter.trim().is_empty() { return true; }
        let f = self.filter.to_lowercase();
        row.name.to_lowercase().contains(&f)
            || row.path.to_lowercase().contains(&f)
            || row.description.to_lowercase().contains(&f)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &ScheduledTask) -> bool { false }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &ScheduledTask, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let color = state_color(&row.state);
                let icon = match row.state.as_str() {
                    "Ready" => icons::STATUS_READY,
                    "Disabled" => icons::STATUS_DISABLED,
                    "Running" => icons::PLAY,
                    "Queued" => icons::STATUS_QUEUED,
                    _ => icons::STATUS_DOT,
                };
                ui.label(RichText::new(format!("{} {}", icon, row.state)).color(color));
            }
            1 => {
                ui.label(RichText::new(&row.name).color(Color32::from_rgb(220, 220, 230)));
            }
            2 => {
                ui.label(RichText::new(&row.path).color(Color32::GRAY).small());
            }
            3 => {
                let text = row.last_run.as_deref().unwrap_or("Never");
                ui.label(RichText::new(text).color(Color32::GRAY).small());
            }
            4 => {
                let text = row.next_run.as_deref().unwrap_or("N/A");
                ui.label(RichText::new(text).color(Color32::GRAY).small());
            }
            5 => {
                let summary: String = row.triggers.iter().take(2).cloned().collect::<Vec<_>>().join("; ");
                let display = if row.triggers.len() > 2 {
                    format!("{} (+{})", summary, row.triggers.len() - 2)
                } else {
                    summary
                };
                ui.label(RichText::new(display).color(Color32::from_rgb(180, 180, 200)).small());
            }
            6 => {
                let summary: String = row.actions.iter().take(2).cloned().collect::<Vec<_>>().join("; ");
                let display = if row.actions.len() > 2 {
                    format!("{} (+{})", summary, row.actions.len() - 2)
                } else {
                    summary
                };
                ui.label(RichText::new(display).color(Color32::from_rgb(180, 180, 200)).small());
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut ScheduledTask,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        row: &ScheduledTask,
        _column: usize,
        resp: &egui::Response,
    ) -> Option<Box<ScheduledTask>> {
        if resp.double_clicked() {
            if let Some(tx) = &self.action_tx {
                let _ = tx.try_send(TaskSchedulerAction::ShowDetail(row.clone()));
            }
        }
        None
    }

    fn set_cell_value(&mut self, src: &ScheduledTask, dst: &mut ScheduledTask, column: usize) {
        match column {
            0 => dst.state = src.state.clone(),
            1 => dst.name = src.name.clone(),
            2 => dst.path = src.path.clone(),
            3 => dst.last_run = src.last_run.clone(),
            4 => dst.next_run = src.next_run.clone(),
            5 => dst.triggers = src.triggers.clone(),
            6 => dst.actions = src.actions.clone(),
            _ => {}
        }
    }

    fn compare_cell(&self, l: &ScheduledTask, r: &ScheduledTask, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.state.cmp(&r.state),
            1 => l.name.to_lowercase().cmp(&r.name.to_lowercase()),
            2 => l.path.to_lowercase().cmp(&r.path.to_lowercase()),
            3 => l.last_run.cmp(&r.last_run),
            4 => l.next_run.cmp(&r.next_run),
            5 => l.triggers.len().cmp(&r.triggers.len()),
            6 => l.actions.len().cmp(&r.actions.len()),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> ScheduledTask {
        ScheduledTask {
            name: String::new(),
            path: String::new(),
            state: String::new(),
            last_run: None,
            next_run: None,
            description: String::new(),
            triggers: Vec::new(),
            actions: Vec::new(),
        }
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(80.).at_most(110.),
            1 => base.at_least(200.).clip(true).resizable(true),
            2 => base.at_least(150.).clip(true).resizable(true),
            3 => base.at_least(130.).at_most(170.).resizable(true),
            4 => base.at_least(130.).at_most(170.).resizable(true),
            5 => base.at_least(150.).clip(true).resizable(true),
            6 => base.at_least(150.).clip(true).resizable(true),
            _ => base,
        }
    }

    fn custom_context_menu_items(
        &mut self,
        _context: &UiActionContext,
        selection: &SelectionSnapshot<'_, ScheduledTask>,
    ) -> Vec<CustomMenuItem> {
        let has_selection = !selection.selected_rows.is_empty();
        let first = selection.selected_rows.first().map(|(_, r)| r);
        let is_disabled = first.map(|r| r.state == "Disabled").unwrap_or(false);
        let is_ready = first.map(|r| r.state == "Ready").unwrap_or(false);

        let mut items = Vec::new();
        items.push(CustomMenuItem::new("enable", "Enable").icon(icons::STATUS_READY).enabled(has_selection && is_disabled));
        items.push(CustomMenuItem::new("disable", "Disable").icon(icons::STATUS_DISABLED).enabled(has_selection && is_ready));
        items.push(CustomMenuItem::new("run_now", "Run Now").icon(icons::PLAY).enabled(has_selection && is_ready));
        items.push(CustomMenuItem::new("detail", "View Details").icon("?").enabled(has_selection));
        items.push(CustomMenuItem::new("refresh", "Refresh").icon(icons::REFRESH).enabled(true));
        items
    }

    fn on_custom_action_ex(
        &mut self,
        action_id: &'static str,
        ctx: &CustomActionContext<'_, ScheduledTask>,
        _editor: &mut CustomActionEditor<ScheduledTask>,
    ) {
        let Some(tx) = &self.action_tx else { return };
        let first = ctx.selection.selected_rows.first().map(|(_, r)| r);

        match action_id {
            "enable" => {
                if let Some(row) = first {
                    let full_path = format!("{}{}", row.path, row.name);
                    let _ = tx.try_send(TaskSchedulerAction::Enable(full_path));
                }
            }
            "disable" => {
                if let Some(row) = first {
                    let full_path = format!("{}{}", row.path, row.name);
                    let _ = tx.try_send(TaskSchedulerAction::Disable(full_path));
                }
            }
            "run_now" => {
                if let Some(row) = first {
                    let full_path = format!("{}{}", row.path, row.name);
                    let _ = tx.try_send(TaskSchedulerAction::RunNow(full_path));
                }
            }
            "detail" => {
                if let Some(row) = first {
                    let _ = tx.try_send(TaskSchedulerAction::ShowDetail((*row).clone()));
                }
            }
            "refresh" => {
                let _ = tx.try_send(TaskSchedulerAction::Refresh);
            }
            _ => {}
        }
    }
}

fn state_color(state: &str) -> Color32 {
    match state {
        "Ready" => Color32::GREEN,
        "Running" => Color32::from_rgb(100, 180, 255),
        "Disabled" => Color32::from_rgb(180, 80, 80),
        "Queued" => Color32::YELLOW,
        _ => Color32::GRAY,
    }
}

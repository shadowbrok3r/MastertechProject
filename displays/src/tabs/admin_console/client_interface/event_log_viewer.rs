use eframe::egui::{
    self, Color32, ComboBox, KeyboardShortcut, RichText, ScrollArea, Sense,
    TextEdit, Ui, Widget, scroll_area,
};
use crossbeam::channel::{Sender, Receiver};
use egui_data_table::{
    viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext},
    DataTable, Renderer, RowViewer, UiAction,
};
use egui_extras::Column as TableColumnConfig;
use serde::Serialize;
use crate::{Cmd, EventLogEntry};
use crate::ui_tools::icons;

const NUM_COLUMNS: usize = 5;

#[derive(Debug, Clone)]
pub enum EventLogAction {
    ShowDetail(usize),
    Refresh,
}

#[derive(Serialize)]
pub struct EventLogRowViewer {
    pub filter: String,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<EventLogAction>>,
}

impl Default for EventLogRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

pub struct EventLogCodec;

impl RowCodec<EventLogEntry> for EventLogCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &EventLogEntry, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&row.level),
            1 => dst.push_str(&row.time),
            2 => dst.push_str(&row.source),
            3 => dst.push_str(&row.event_id.to_string()),
            4 => dst.push_str(&row.message),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src: &str,
        column: usize,
        row: &mut EventLogEntry,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => row.level = src.to_string(),
            1 => row.time = src.to_string(),
            2 => row.source = src.to_string(),
            3 => row.event_id = src.parse().unwrap_or(0),
            4 => row.message = src.to_string(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> EventLogEntry {
        EventLogEntry {
            level: String::new(),
            time: String::new(),
            source: String::new(),
            event_id: 0,
            message: String::new(),
        }
    }
}

pub struct EventLogViewer {
    pub table: DataTable<EventLogEntry>,
    pub viewer: EventLogRowViewer,
    pub action_rx: Receiver<EventLogAction>,
    action_tx: Sender<EventLogAction>,
    pub loading: bool,
    pub selected_log: String,
    pub max_entries: u32,
    pub level_filter: String,
    pub detail_message: Option<String>,
    pub entries: Vec<EventLogEntry>,
}

impl Default for EventLogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLogViewer {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut viewer = EventLogRowViewer::default();
        viewer.action_tx = Some(action_tx.clone());

        Self {
            table: DataTable::new(),
            viewer,
            action_rx,
            action_tx,
            loading: false,
            selected_log: "System".to_string(),
            max_entries: 200,
            level_filter: "All".to_string(),
            detail_message: None,
            entries: Vec::new(),
        }
    }

    pub fn set_entries(&mut self, entries: Vec<EventLogEntry>) {
        self.entries = entries.clone();
        self.table.replace(entries);
        self.loading = false;
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                EventLogAction::ShowDetail(idx) => {
                    if let Some(entry) = self.entries.get(idx) {
                        self.detail_message = Some(format!(
                            "[{}] {} - Event ID: {}\nSource: {}\n\n{}",
                            entry.level, entry.time, entry.event_id, entry.source, entry.message
                        ));
                    }
                }
                EventLogAction::Refresh => {
                    self.loading = true;
                    self.table.clear();
                    self.entries.clear();
                    let filter = if self.level_filter == "All" { None } else { Some(self.level_filter.clone()) };
                    let _ = cmd_tx.try_send(Cmd::ReadEventLog {
                        log_name: self.selected_log.clone(),
                        max_entries: self.max_entries,
                        level_filter: filter,
                    });
                }
            }
        }

        // Toolbar
        ui.horizontal(|ui| {
            ui.label(RichText::new("Log:").strong());
            let prev_log = self.selected_log.clone();
            ComboBox::from_id_salt("event_log_name")
                .selected_text(&self.selected_log)
                .show_ui(ui, |ui| {
                    for log in &["System", "Application", "Security", "Setup"] {
                        ui.selectable_value(&mut self.selected_log, log.to_string(), *log);
                    }
                });

            ui.label("Level:");
            ComboBox::from_id_salt("event_log_level")
                .selected_text(&self.level_filter)
                .show_ui(ui, |ui| {
                    for level in &["All", "Critical", "Error", "Warning", "Information"] {
                        ui.selectable_value(&mut self.level_filter, level.to_string(), *level);
                    }
                });

            ui.label("Max:");
            let mut max_str = self.max_entries.to_string();
            if TextEdit::singleline(&mut max_str).desired_width(50.).show(ui).response.changed() {
                if let Ok(v) = max_str.parse::<u32>() {
                    self.max_entries = v.clamp(10, 5000);
                }
            }

            if ui.button("Refresh").clicked() || prev_log != self.selected_log {
                self.loading = true;
                self.table.clear();
                self.entries.clear();
                let filter = if self.level_filter == "All" { None } else { Some(self.level_filter.clone()) };
                let _ = cmd_tx.try_send(Cmd::ReadEventLog {
                    log_name: self.selected_log.clone(),
                    max_entries: self.max_entries,
                    level_filter: filter,
                });
            }

            ui.label("Filter:");
            TextEdit::singleline(&mut self.viewer.filter)
                .hint_text("Search events...")
                .desired_width(150.)
                .show(ui);
        });

        ui.add_space(4.);

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading event log...");
            });
            return;
        }

        if self.entries.is_empty() {
            ui.label(RichText::new("No events loaded. Click Refresh to fetch events.").italics().color(Color32::GRAY));
            return;
        }

        // Detail modal
        if let Some(detail) = self.detail_message.clone() {
            egui::Window::new("Event Detail")
                .collapsible(false)
                .resizable(true)
                .default_width(500.)
                .show(ui.ctx(), |ui| {
                    ScrollArea::vertical().max_height(400.).show(ui, |ui| {
                        ui.label(&detail);
                    });
                    if ui.button("Close").clicked() {
                        self.detail_message = None;
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

impl RowViewer<EventLogEntry> for EventLogRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<EventLogEntry>> {
        Some(EventLogCodec)
    }

    fn num_columns(&mut self) -> usize { NUM_COLUMNS }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Level", "Time", "Source", "Event ID", "Message"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &EventLogEntry) -> bool {
        if self.filter.trim().is_empty() { return true; }
        let f = self.filter.to_lowercase();
        row.source.to_lowercase().contains(&f)
            || row.message.to_lowercase().contains(&f)
            || row.event_id.to_string().contains(&f)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &EventLogEntry) -> bool { false }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &EventLogEntry, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let (icon, color) = match row.level.as_str() {
                    "Critical" => (icons::CRITICAL, Color32::from_rgb(255, 60,  60)),
                    "Error" => (icons::STATUS_ERR, Color32::from_rgb(255, 100, 100)),
                    "Warning" => (icons::STATUS_WARN, Color32::YELLOW),
                    "Information" => (icons::INFO, Color32::from_rgb(100, 180, 255)),
                    "Verbose" => (icons::CLIPBOARD, Color32::GRAY),
                    _ => (icons::STATUS_DOT, Color32::GRAY),
                };
                ui.label(RichText::new(format!("{} {}", icon, row.level)).color(color));
            }
            1 => {
                let display = format_event_time(&row.time);
                ui.label(RichText::new(display).color(Color32::GRAY).small());
            }
            2 => {
                ui.label(RichText::new(&row.source).color(Color32::from_rgb(200, 200, 220)));
            }
            3 => {
                ui.label(RichText::new(row.event_id.to_string()).color(Color32::from_rgb(180, 180, 200)));
            }
            4 => {
                let truncated: String = row.message.chars().take(120).collect();
                let label = if row.message.len() > 120 {
                    format!("{}...", truncated)
                } else {
                    truncated
                };
                egui::Label::new(RichText::new(label).color(Color32::from_rgb(200, 200, 200)).small())
                    .sense(Sense::click())
                    .ui(ui);
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut EventLogEntry,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        row: &EventLogEntry,
        column: usize,
        resp: &egui::Response,
    ) -> Option<Box<EventLogEntry>> {
        if column == 4 {
            resp.clone().on_hover_text(&row.message);
        }
        if resp.double_clicked() {
            if let Some(tx) = &self.action_tx {
                let _ = tx.try_send(EventLogAction::ShowDetail(0));
            }
        }
        None
    }

    fn set_cell_value(&mut self, src: &EventLogEntry, dst: &mut EventLogEntry, column: usize) {
        match column {
            0 => dst.level = src.level.clone(),
            1 => dst.time = src.time.clone(),
            2 => dst.source = src.source.clone(),
            3 => dst.event_id = src.event_id,
            4 => dst.message = src.message.clone(),
            _ => {}
        }
    }

    fn compare_cell(&self, l: &EventLogEntry, r: &EventLogEntry, column: usize) -> std::cmp::Ordering {
        match column {
            0 => level_severity(&l.level).cmp(&level_severity(&r.level)),
            1 => l.time.cmp(&r.time),
            2 => l.source.to_lowercase().cmp(&r.source.to_lowercase()),
            3 => l.event_id.cmp(&r.event_id),
            4 => l.message.to_lowercase().cmp(&r.message.to_lowercase()),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> EventLogEntry {
        EventLogEntry {
            level: String::new(),
            time: String::new(),
            source: String::new(),
            event_id: 0,
            message: String::new(),
        }
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(90.).at_most(120.),
            1 => base.at_least(140.).at_most(170.).resizable(true),
            2 => base.at_least(150.).clip(true).resizable(true),
            3 => base.at_least(70.).at_most(90.),
            4 => base.at_least(300.).clip(true).resizable(true),
            _ => base,
        }
    }
}

fn level_severity(level: &str) -> u8 {
    match level {
        "Critical" => 0,
        "Error" => 1,
        "Warning" => 2,
        "Information" => 3,
        "Verbose" => 4,
        _ => 5,
    }
}

fn format_event_time(time: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time) {
        dt.format("%b %d, %Y  %I:%M:%S %p").to_string()
    } else {
        time.chars().take(19).collect()
    }
}

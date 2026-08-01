use eframe::egui::{
    self, Align, Color32, Frame, KeyboardShortcut, Layout, Margin, RichText, ScrollArea,
    Stroke, TextEdit, Ui, Widget, scroll_area, CornerRadius,
};
use crossbeam::channel::{Sender, Receiver};
use egui_data_table::{
    viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext, CustomActionContext, CustomActionEditor},
    CustomMenuItem, DataTable, Renderer, RowViewer, SelectionSnapshot, UiAction,
};
use egui_extras::Column as TableColumnConfig;
use crate::ui_tools::icons;
use serde::Serialize;
use crate::{Cmd, RegistryEdit, RegistryKeyInfo, RegistryValueEntry, ui_tools::theme};
use std::collections::{BTreeMap, BTreeSet};

const NUM_VALUE_COLUMNS: usize = 3;

#[derive(Debug, Clone)]
pub enum RegistryAction {
    EditValue { name: String, kind: String, data: String },
    DeleteValue(String),
    NewValue { kind: String },
    Refresh,
}

#[derive(Serialize)]
pub struct RegistryValueRowViewer {
    pub filter: String,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<RegistryAction>>,
}

impl Default for RegistryValueRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

pub struct RegistryValueCodec;

impl RowCodec<RegistryValueEntry> for RegistryValueCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &RegistryValueEntry, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&row.name),
            1 => dst.push_str(&row.kind),
            2 => dst.push_str(&row.data),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src: &str,
        column: usize,
        row: &mut RegistryValueEntry,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => row.name = src.to_string(),
            1 => row.kind = src.to_string(),
            2 => row.data = src.to_string(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> RegistryValueEntry {
        RegistryValueEntry {
            name: String::new(),
            kind: String::new(),
            data: String::new(),
        }
    }
}

#[derive(Clone)]
struct RegistryTreeNode {
    name: String,
    children_loaded: bool,
    children: Vec<String>,
    subkey_count: u32,
}

pub struct RegistryEditor {
    pub values_table: DataTable<RegistryValueEntry>,
    pub values_viewer: RegistryValueRowViewer,
    pub action_rx: Receiver<RegistryAction>,

    tree_nodes: BTreeMap<String, RegistryTreeNode>,
    expanded: BTreeSet<String>,
    pub selected_key: String,
    pub loading: bool,
    pub loading_values: bool,

    pub pending_edits: Vec<RegistryEdit>,
    original_values: Vec<RegistryValueEntry>,
    pub show_diff_modal: bool,
    pub backup_pending: bool,
    pub backup_path: Option<String>,
    pub status_message: Option<(String, bool)>,

    edit_dialog: Option<ValueEditDialog>,
    new_value_dialog: Option<NewValueDialog>,
}

#[derive(Clone)]
struct ValueEditDialog {
    name: String,
    kind: String,
    data: String,
    original_data: String,
}

struct NewValueDialog {
    name: String,
    kind: String,
    data: String,
}

impl Default for RegistryEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryEditor {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut viewer = RegistryValueRowViewer::default();
        viewer.action_tx = Some(action_tx);

        let hives = vec![
            ("HKEY_LOCAL_MACHINE", "HKLM"),
            ("HKEY_CURRENT_USER", "HKCU"),
            ("HKEY_CLASSES_ROOT", "HKCR"),
            ("HKEY_USERS", "HKU"),
            ("HKEY_CURRENT_CONFIG", "HKCC"),
        ];

        let mut tree_nodes = BTreeMap::new();
        let root_children: Vec<String> = hives.iter().map(|(full, _)| full.to_string()).collect();

        for (full, short) in &hives {
            tree_nodes.insert(
                full.to_string(),
                RegistryTreeNode {
                    name: format!("{} ({})", short, full),
                    children_loaded: false,
                    children: Vec::new(),
                    subkey_count: 0,
                },
            );
        }

        tree_nodes.insert(
            "ROOT".to_string(),
            RegistryTreeNode {
                name: "Computer".to_string(),
                children_loaded: true,
                children: root_children,
                subkey_count: 5,
            },
        );

        let mut expanded = BTreeSet::new();
        expanded.insert("ROOT".to_string());

        Self {
            values_table: DataTable::new(),
            values_viewer: viewer,
            action_rx,
            tree_nodes,
            expanded,
            selected_key: String::new(),
            loading: false,
            loading_values: false,
            pending_edits: Vec::new(),
            original_values: Vec::new(),
            show_diff_modal: false,
            backup_pending: false,
            backup_path: None,
            status_message: None,
            edit_dialog: None,
            new_value_dialog: None,
        }
    }

    pub fn set_key_data(
        &mut self,
        path: String,
        subkeys: Vec<RegistryKeyInfo>,
        values: Vec<RegistryValueEntry>,
    ) {
        if let Some(node) = self.tree_nodes.get_mut(&path) {
            node.children_loaded = true;
            node.children = subkeys.iter().map(|k| k.path.clone()).collect();
            node.subkey_count = subkeys.len() as u32;
        }

        for key in &subkeys {
            self.tree_nodes.entry(key.path.clone()).or_insert_with(|| RegistryTreeNode {
                name: key.name.clone(),
                children_loaded: false,
                children: Vec::new(),
                subkey_count: key.subkey_count,
            });
        }

        if self.selected_key == path {
            self.original_values = values.clone();
            self.values_table.replace(values);
            self.loading_values = false;
        }

        self.loading = false;
    }

    pub fn set_backup_result(&mut self, success: bool, backup_path: String, message: String) {
        if success && self.backup_pending {
            self.backup_path = Some(backup_path);
            self.backup_pending = false;
            self.status_message = Some((format!("Backup created: {}", message), true));
        } else if !success {
            self.backup_pending = false;
            self.status_message = Some((format!("Backup failed: {}", message), false));
        }
    }

    pub fn set_edit_result(&mut self, success: bool, message: String) {
        self.status_message = Some((message, success));
        if success {
            self.pending_edits.clear();
        }
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                RegistryAction::EditValue { name, kind, data } => {
                    self.edit_dialog = Some(ValueEditDialog {
                        original_data: data.clone(),
                        name,
                        kind,
                        data,
                    });
                }
                RegistryAction::DeleteValue(name) => {
                    self.pending_edits.push(RegistryEdit::DeleteValue {
                        path: self.selected_key.clone(),
                        name,
                    });
                }
                RegistryAction::NewValue { kind } => {
                    self.new_value_dialog = Some(NewValueDialog {
                        name: String::new(),
                        kind,
                        data: String::new(),
                    });
                }
                RegistryAction::Refresh => {
                    if !self.selected_key.is_empty() {
                        self.loading_values = true;
                        let _ = cmd_tx.try_send(Cmd::ListRegistryKeys(self.selected_key.clone()));
                    }
                }
            }
        }

        // Edit value dialog
        if let Some(dialog) = &mut self.edit_dialog.clone() {
            let mut open = true;
            egui::Window::new("Edit Value")
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    egui::Grid::new("edit_value_grid").num_columns(2).show(ui, |ui| {
                        ui.label("Name:");
                        ui.label(&dialog.name);
                        ui.end_row();

                        ui.label("Type:");
                        ui.label(&dialog.kind);
                        ui.end_row();

                        ui.label("Data:");
                        if let Some(d) = &mut self.edit_dialog {
                            TextEdit::multiline(&mut d.data).desired_rows(3).desired_width(300.).show(ui);
                        }
                        ui.end_row();
                    });

                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            if let Some(d) = &self.edit_dialog {
                                if d.data != d.original_data {
                                    self.pending_edits.push(RegistryEdit::SetValue {
                                        path: self.selected_key.clone(),
                                        name: d.name.clone(),
                                        kind: d.kind.clone(),
                                        data: d.data.clone(),
                                    });
                                }
                            }
                            self.edit_dialog = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.edit_dialog = None;
                        }
                    });
                });
            if !open { self.edit_dialog = None; }
        }

        // New value dialog
        if let Some(_) = &self.new_value_dialog {
            let mut open = true;
            egui::Window::new("New Value")
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    if let Some(d) = &mut self.new_value_dialog {
                        egui::Grid::new("new_value_grid").num_columns(2).show(ui, |ui| {
                            ui.label("Name:");
                            TextEdit::singleline(&mut d.name).desired_width(200.).show(ui);
                            ui.end_row();

                            ui.label("Type:");
                            ui.label(&d.kind);
                            ui.end_row();

                            ui.label("Data:");
                            TextEdit::multiline(&mut d.data).desired_rows(3).desired_width(300.).show(ui);
                            ui.end_row();
                        });
                    }

                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            if let Some(d) = &self.new_value_dialog {
                                if !d.name.is_empty() {
                                    self.pending_edits.push(RegistryEdit::SetValue {
                                        path: self.selected_key.clone(),
                                        name: d.name.clone(),
                                        kind: d.kind.clone(),
                                        data: d.data.clone(),
                                    });
                                }
                            }
                            self.new_value_dialog = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.new_value_dialog = None;
                        }
                    });
                });
            if !open { self.new_value_dialog = None; }
        }

        // Diff modal
        if self.show_diff_modal {
            egui::Window::new("Review Pending Changes")
                .collapsible(false)
                .resizable(true)
                .default_width(600.)
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new(format!("{} pending change(s)", self.pending_edits.len())).strong());
                    ui.add_space(8.);

                    ScrollArea::vertical().max_height(400.).show(ui, |ui| {
                        for (i, edit) in self.pending_edits.iter().enumerate() {
                            ui.group(|ui| {
                                match edit {
                                    RegistryEdit::SetValue { path: _, name, kind, data } => {
                                        let original = self.original_values.iter()
                                            .find(|v| v.name == *name)
                                            .map(|v| v.data.as_str())
                                            .unwrap_or("<new>");
                                        ui.label(RichText::new(format!("#{} SET {} [{}]", i + 1, name, kind)).strong());
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Before:").color(ui.style().visuals.error_fg_color));
                                            ui.label(original);
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("After:").color(Color32::LIGHT_GREEN));
                                            ui.label(data);
                                        });
                                    }
                                    RegistryEdit::DeleteValue { path: _, name } => {
                                        let original = self.original_values.iter()
                                            .find(|v| v.name == *name)
                                            .map(|v| v.data.as_str())
                                            .unwrap_or("?");
                                        ui.label(RichText::new(format!("#{} DELETE {}", i + 1, name)).strong().color(ui.style().visuals.error_fg_color));
                                        ui.label(format!("Value: {}", original));
                                    }
                                    RegistryEdit::CreateKey { path } => {
                                        ui.label(RichText::new(format!("#{} CREATE KEY {}", i + 1, path)).strong().color(Color32::LIGHT_GREEN));
                                    }
                                    RegistryEdit::DeleteKey { path } => {
                                        ui.label(RichText::new(format!("#{} DELETE KEY {}", i + 1, path)).strong().color(ui.style().visuals.error_fg_color));
                                    }
                                }
                            });
                            ui.add_space(4.);
                        }
                    });

                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Confirm & Apply").color(Color32::LIGHT_GREEN)).clicked() {
                            self.show_diff_modal = false;
                            self.backup_pending = true;
                            let _ = cmd_tx.try_send(Cmd::BackupRegistryKey(self.selected_key.clone()));
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_diff_modal = false;
                        }
                    });
                });
        }

        // After backup completes, commit edits
        if !self.backup_pending && self.backup_path.is_some() && !self.pending_edits.is_empty() {
            let edits = self.pending_edits.clone();
            let _ = cmd_tx.try_send(Cmd::CommitRegistryEdits(edits));
            self.backup_path = None;
        }

        // Top toolbar
        ui.horizontal(|ui| {
            ui.label(RichText::new("Registry:").strong());
            ui.label(RichText::new(if self.selected_key.is_empty() { "Select a key" } else { &self.selected_key }).color(Color32::from_rgb(180, 200, 220)));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some((msg, success)) = &self.status_message {
                    let color = if *success { Color32::LIGHT_GREEN } else { ui.style().visuals.error_fg_color };
                    ui.colored_label(color, msg);
                }

                let has_edits = !self.pending_edits.is_empty();
                if has_edits {
                    ui.label(RichText::new(format!("{} pending", self.pending_edits.len())).color(theme::warn(ui)));
                }

                let submit_btn = ui.add_enabled(has_edits, egui::Button::new(
                    RichText::new("Submit Changes").color(if has_edits { Color32::LIGHT_GREEN } else { theme::weak_text(ui) })
                ));
                if submit_btn.clicked() {
                    self.show_diff_modal = true;
                }

                if has_edits && ui.button(RichText::new("Discard").color(ui.style().visuals.error_fg_color).small()).clicked() {
                    self.pending_edits.clear();
                }
            });
        });

        ui.add_space(4.);

        let stroke = Stroke::new(1.0_f32, Color32::from_rgb(40, 40, 48));
        let radius = CornerRadius::same(4);

        // Left: tree, Right: values
        let sidebar_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 24))
            .inner_margin(Margin::same(8))
            .corner_radius(radius)
            .stroke(stroke);

        eframe::egui::Panel::left("RegistryTree")
            .frame(sidebar_frame)
            .resizable(true)
            .default_size(280.)
            .min_size(200.)
            .max_size(500.)
            .show(ui, |ui| {
                ui.label(RichText::new("Registry Keys").strong().color(Color32::LIGHT_GRAY));
                ui.separator();

                let mut navigate_to: Option<String> = None;
                let mut expand_key: Option<String> = None;

                ScrollArea::vertical().show(ui, |ui| {
                    let root_children = self.tree_nodes.get("ROOT")
                        .map(|n| n.children.clone())
                        .unwrap_or_default();

                    for child_path in &root_children {
                        self.render_tree_node(ui, child_path, 0, &mut navigate_to, &mut expand_key);
                    }
                });

                if let Some(path) = expand_key {
                    self.expanded.insert(path.clone());
                    if let Some(node) = self.tree_nodes.get(&path) {
                        if !node.children_loaded {
                            self.loading = true;
                            let _ = cmd_tx.try_send(Cmd::ListRegistryKeys(path));
                        }
                    }
                }

                if let Some(path) = navigate_to {
                    self.selected_key = path.clone();
                    self.loading_values = true;
                    self.pending_edits.clear();
                    let _ = cmd_tx.try_send(Cmd::ListRegistryKeys(path));
                }
            });

        // Right panel: values
        egui::CentralPanel::default().show(ui, |ui| {
            if self.selected_key.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Select a registry key to view its values").italics().color(Color32::GRAY));
                });
                return;
            }

            if self.loading_values {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading values...");
                });
                return;
            }

            ui.horizontal(|ui| {
                ui.label("Filter:");
                TextEdit::singleline(&mut self.values_viewer.filter)
                    .hint_text("Search values...")
                    .desired_width(200.)
                    .show(ui);
            });
            ui.add_space(2.);

            ScrollArea::horizontal()
                .auto_shrink(false)
                .show(ui, |ui| {
                    Renderer::new(&mut self.values_table, &mut self.values_viewer)
                        .with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui)
                });
        });
    }

    fn render_tree_node(
        &self,
        ui: &mut Ui,
        path: &str,
        depth: usize,
        navigate_to: &mut Option<String>,
        expand_key: &mut Option<String>,
    ) {
        let Some(node) = self.tree_nodes.get(path) else { return };
        let is_expanded = self.expanded.contains(path);
        let is_selected = self.selected_key == path;
        let has_children = node.subkey_count > 0 || !node.children.is_empty();

        let indent = depth as f32 * 16.;
        ui.horizontal(|ui| {
            ui.add_space(indent);

            let arrow = if !has_children {
                "   "
            } else if is_expanded {
                " v "
            } else {
                " > "
            };

            if has_children {
                if ui.selectable_label(false, arrow).clicked() {
                    if is_expanded {
                        // collapse handled by caller
                    } else {
                        *expand_key = Some(path.to_string());
                    }
                }
            } else {
                ui.label(arrow);
            }

            let label_color = if is_selected {
                Color32::from_rgb(100, 180, 255)
            } else {
                Color32::from_rgb(220, 220, 230)
            };
            let icon = if is_expanded { "/" } else { "." };
            let resp = ui.selectable_label(is_selected, RichText::new(format!("{} {}", icon, node.name)).color(label_color));
            if resp.clicked() {
                *navigate_to = Some(path.to_string());
            }
        });

        if is_expanded {
            let children = node.children.clone();
            for child_path in &children {
                self.render_tree_node(ui, child_path, depth + 1, navigate_to, expand_key);
            }
        }
    }
}

impl RowViewer<RegistryValueEntry> for RegistryValueRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<RegistryValueEntry>> {
        Some(RegistryValueCodec)
    }

    fn num_columns(&mut self) -> usize { NUM_VALUE_COLUMNS }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Name", "Type", "Data"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &RegistryValueEntry) -> bool {
        if self.filter.trim().is_empty() { return true; }
        let f = self.filter.to_lowercase();
        row.name.to_lowercase().contains(&f)
            || row.data.to_lowercase().contains(&f)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &RegistryValueEntry) -> bool { false }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &RegistryValueEntry, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let icon = if row.name.is_empty() || row.name == "(Default)" {
                    icons::HOME
                } else {
                    icons::CLIPBOARD
                };
                let display_name = if row.name.is_empty() { "(Default)" } else { &row.name };
                ui.label(RichText::new(format!("{} {}", icon, display_name)).color(Color32::from_rgb(220, 220, 230)));
            }
            1 => {
                let color = match row.kind.as_str() {
                    "REG_SZ" | "REG_EXPAND_SZ" => Color32::from_rgb(130, 200, 130),
                    "REG_DWORD" | "REG_QWORD" => Color32::from_rgb(130, 170, 255),
                    "REG_BINARY" => Color32::from_rgb(255, 180, 100),
                    "REG_MULTI_SZ" => Color32::from_rgb(200, 160, 255),
                    _ => Color32::GRAY,
                };
                ui.label(RichText::new(&row.kind).color(color).small());
            }
            2 => {
                let truncated: String = row.data.chars().take(200).collect();
                let display = if row.data.len() > 200 {
                    format!("{}...", truncated)
                } else {
                    truncated
                };
                ui.label(RichText::new(display).color(Color32::from_rgb(200, 200, 200)));
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut RegistryValueEntry,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        row: &RegistryValueEntry,
        _column: usize,
        resp: &egui::Response,
    ) -> Option<Box<RegistryValueEntry>> {
        if resp.double_clicked() {
            if let Some(tx) = &self.action_tx {
                let _ = tx.try_send(RegistryAction::EditValue {
                    name: row.name.clone(),
                    kind: row.kind.clone(),
                    data: row.data.clone(),
                });
            }
        }
        None
    }

    fn set_cell_value(&mut self, src: &RegistryValueEntry, dst: &mut RegistryValueEntry, column: usize) {
        match column {
            0 => dst.name = src.name.clone(),
            1 => dst.kind = src.kind.clone(),
            2 => dst.data = src.data.clone(),
            _ => {}
        }
    }

    fn compare_cell(&self, l: &RegistryValueEntry, r: &RegistryValueEntry, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.name.to_lowercase().cmp(&r.name.to_lowercase()),
            1 => l.kind.cmp(&r.kind),
            2 => l.data.cmp(&r.data),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> RegistryValueEntry {
        RegistryValueEntry {
            name: String::new(),
            kind: String::new(),
            data: String::new(),
        }
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(180.).clip(true).resizable(true),
            1 => base.at_least(100.).at_most(140.),
            2 => base.at_least(300.).clip(true).resizable(true),
            _ => base,
        }
    }

    fn custom_context_menu_items(
        &mut self,
        _context: &UiActionContext,
        selection: &SelectionSnapshot<'_, RegistryValueEntry>,
    ) -> Vec<CustomMenuItem> {
        let has_selection = !selection.selected_rows.is_empty();

        let mut items = Vec::new();
        items.push(CustomMenuItem::new("edit", "Edit Value").icon(icons::EDIT).enabled(has_selection));
        items.push(CustomMenuItem::new("delete", "Delete Value").icon(icons::CLOSE).enabled(has_selection));
        items.push(CustomMenuItem::new("new_sz", "New String Value (REG_SZ)").icon("+").enabled(true));
        items.push(CustomMenuItem::new("new_dword", "New DWORD Value").icon("+").enabled(true));
        items.push(CustomMenuItem::new("new_qword", "New QWORD Value").icon("+").enabled(true));
        items.push(CustomMenuItem::new("new_binary", "New Binary Value").icon("+").enabled(true));
        items.push(CustomMenuItem::new("new_multi_sz", "New Multi-String (REG_MULTI_SZ)").icon("+").enabled(true));
        items.push(CustomMenuItem::new("new_expand_sz", "New Expandable String (REG_EXPAND_SZ)").icon("+").enabled(true));
        items.push(CustomMenuItem::new("refresh", "Refresh").icon(icons::REFRESH).enabled(true));
        items
    }

    fn on_custom_action_ex(
        &mut self,
        action_id: &'static str,
        ctx: &CustomActionContext<'_, RegistryValueEntry>,
        _editor: &mut CustomActionEditor<RegistryValueEntry>,
    ) {
        let Some(tx) = &self.action_tx else { return };
        let first = ctx.selection.selected_rows.first().map(|(_, r)| r);

        match action_id {
            "edit" => {
                if let Some(row) = first {
                    let _ = tx.try_send(RegistryAction::EditValue {
                        name: row.name.clone(),
                        kind: row.kind.clone(),
                        data: row.data.clone(),
                    });
                }
            }
            "delete" => {
                if let Some(row) = first {
                    let _ = tx.try_send(RegistryAction::DeleteValue(row.name.clone()));
                }
            }
            "new_sz" => { let _ = tx.try_send(RegistryAction::NewValue { kind: "REG_SZ".to_string() }); }
            "new_dword" => { let _ = tx.try_send(RegistryAction::NewValue { kind: "REG_DWORD".to_string() }); }
            "new_qword" => { let _ = tx.try_send(RegistryAction::NewValue { kind: "REG_QWORD".to_string() }); }
            "new_binary" => { let _ = tx.try_send(RegistryAction::NewValue { kind: "REG_BINARY".to_string() }); }
            "new_multi_sz" => { let _ = tx.try_send(RegistryAction::NewValue { kind: "REG_MULTI_SZ".to_string() }); }
            "new_expand_sz" => { let _ = tx.try_send(RegistryAction::NewValue { kind: "REG_EXPAND_SZ".to_string() }); }
            "refresh" => { let _ = tx.try_send(RegistryAction::Refresh); }
            _ => {}
        }
    }
}

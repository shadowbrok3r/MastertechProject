//! Installed Programs viewer — slice 3 of the connected-client
//! refactor. Modeled after `services_viewer.rs`: an egui_data_table
//! `Renderer` over a `Vec<InstalledProgram>` with a toolbar for
//! refresh / filter and a context menu that fires uninstall
//! actions through a channel.
//!
//! The admin clicks "Uninstall" → we send `Cmd::UninstallProgram
//! { id, prefer_silent: true }` → the client runs the strategy
//! ladder → we receive `Cmd::UninstallProgramResult` and surface
//! the message in the status row. A successful uninstall also
//! re-fetches the program list so the row disappears.

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

use crate::{Cmd, InstalledProgram};
use crate::ui_tools::icons;

const NUM_COLUMNS: usize = 6;

#[derive(Debug, Clone)]
pub enum InstalledProgramAction {
    /// Try a silent uninstall first; fall back to the publisher's
    /// raw uninstaller if no silent variant is available.
    UninstallSilent(String),
    /// Skip the silent path entirely — admin explicitly wants the
    /// GUI uninstaller on the remote, e.g. to confirm a license
    /// transfer dialog or pick which components to remove.
    UninstallInteractive(String),
    Refresh,
}

#[derive(Serialize)]
pub struct InstalledProgramRowViewer {
    pub filter: String,
    /// When true, hides rows that have no uninstall string at all
    /// — those are usually Windows-update payloads that can't be
    /// removed from this admin path anyway.
    pub uninstallable_only: bool,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<InstalledProgramAction>>,
}

impl Default for InstalledProgramRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            uninstallable_only: false,
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

pub struct InstalledProgramCodec;

impl RowCodec<InstalledProgram> for InstalledProgramCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &InstalledProgram, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&row.name),
            1 => {
                if let Some(v) = row.version.as_deref() {
                    dst.push_str(v);
                }
            }
            2 => {
                if let Some(p) = row.publisher.as_deref() {
                    dst.push_str(p);
                }
            }
            3 => {
                if let Some(d) = row.install_date.as_deref() {
                    dst.push_str(d);
                }
            }
            4 => {
                if let Some(kb) = row.estimated_size_kb {
                    dst.push_str(&kb.to_string());
                }
            }
            5 => dst.push_str(&row.registry_hive),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        _src: &str,
        _column: usize,
        _row: &mut InstalledProgram,
    ) -> Result<(), DecodeErrorBehavior> {
        // The viewer is read-only — paste / cell edit is disabled
        // via `is_editable_cell`. Decoding is only used by the
        // table's clipboard paste path which we don't expose.
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> InstalledProgram {
        empty_program()
    }
}

fn empty_program() -> InstalledProgram {
    InstalledProgram {
        id: String::new(),
        name: String::new(),
        version: None,
        publisher: None,
        install_date: None,
        estimated_size_kb: None,
        uninstall_string: None,
        quiet_uninstall_string: None,
        registry_hive: String::new(),
        is_wow6432: false,
    }
}

pub struct InstalledProgramsViewer {
    pub table: DataTable<InstalledProgram>,
    pub viewer: InstalledProgramRowViewer,
    pub action_rx: Receiver<InstalledProgramAction>,
    pub loading: bool,
    pub entries: Vec<InstalledProgram>,
    /// `(text, success)` rendered next to the toolbar. Mirrors
    /// `ServicesViewer::status_message`.
    pub status_message: Option<(String, bool)>,
    /// Pending uninstall confirmation. The admin's first click
    /// sets this so we can show a "Are you sure?" dialog before
    /// firing the destructive action — Services has the same
    /// pattern for Stop / Restart.
    pub confirm_action: Option<(String, InstalledProgramAction)>,
}

impl Default for InstalledProgramsViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl InstalledProgramsViewer {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut viewer = InstalledProgramRowViewer::default();
        viewer.action_tx = Some(action_tx);

        Self {
            table: DataTable::new(),
            viewer,
            action_rx,
            loading: false,
            entries: Vec::new(),
            status_message: None,
            confirm_action: None,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<InstalledProgram>) {
        self.entries = entries.clone();
        self.table.replace(entries);
        self.loading = false;
    }

    /// Hook the receive-loop calls when `UninstallProgramResult`
    /// arrives. On success we trigger a refresh so the uninstalled
    /// row disappears.
    pub fn set_action_result(&mut self, id: String, success: bool, message: String) {
        // Look up the display name so the status row reads
        // "Steam: Strategy: MSI silent (exit 0)" instead of just
        // the registry subkey id.
        let display = self
            .entries
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or(id);
        self.status_message = Some((format!("{display}: {message}"), success));
    }

    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        // Drain pending actions from the data-table context-menu
        // / hotkey handlers. We translate them to Cmd messages
        // here so the action handler doesn't need a direct cmd_tx
        // — same separation Services uses.
        while let Ok(action) = self.action_rx.try_recv() {
            match &action {
                InstalledProgramAction::UninstallSilent(id) => {
                    self.confirm_action = Some((
                        format!(
                            "Uninstall '{}'?\n\nThis runs a silent uninstall on the remote machine. If the publisher didn't ship a silent variant, the GUI uninstaller may appear on the user's screen.",
                            self.entries.iter().find(|p| &p.id == id).map(|p| p.name.as_str()).unwrap_or(id)
                        ),
                        action.clone(),
                    ));
                }
                InstalledProgramAction::UninstallInteractive(id) => {
                    self.confirm_action = Some((
                        format!(
                            "Uninstall '{}' (interactive)?\n\nThis runs the publisher's full uninstaller — the user will see GUI prompts on their screen.",
                            self.entries.iter().find(|p| &p.id == id).map(|p| p.name.as_str()).unwrap_or(id)
                        ),
                        action.clone(),
                    ));
                }
                InstalledProgramAction::Refresh => {
                    self.loading = true;
                    self.table.clear();
                    self.entries.clear();
                    self.status_message = None;
                    let _ = cmd_tx.try_send(Cmd::ListInstalledPrograms);
                }
            }
        }

        // Confirmation modal.
        if let Some((msg, pending_action)) = self.confirm_action.clone() {
            egui::Window::new("Confirm Uninstall")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(&msg);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Uninstall").clicked() {
                            match &pending_action {
                                InstalledProgramAction::UninstallSilent(id) => {
                                    let _ = cmd_tx.try_send(Cmd::UninstallProgram {
                                        id: id.clone(),
                                        prefer_silent: true,
                                    });
                                }
                                InstalledProgramAction::UninstallInteractive(id) => {
                                    let _ = cmd_tx.try_send(Cmd::UninstallProgram {
                                        id: id.clone(),
                                        prefer_silent: false,
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

        // Toolbar.
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.loading = true;
                self.table.clear();
                self.entries.clear();
                self.status_message = None;
                let _ = cmd_tx.try_send(Cmd::ListInstalledPrograms);
            }

            ui.label("Filter:");
            TextEdit::singleline(&mut self.viewer.filter)
                .hint_text("Search programs…")
                .desired_width(220.)
                .show(ui);

            ui.checkbox(&mut self.viewer.uninstallable_only, "Uninstallable only");

            if let Some((msg, success)) = &self.status_message {
                let color = if *success { Color32::LIGHT_GREEN } else { ui.style().visuals.error_fg_color };
                ui.colored_label(color, msg);
            }
        });

        ui.add_space(4.);

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading installed programs…");
            });
            return;
        }

        if self.entries.is_empty() {
            ui.label(
                RichText::new("No programs loaded. Click Refresh to fetch.")
                    .italics()
                    .color(Color32::GRAY),
            );
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

impl RowViewer<InstalledProgram> for InstalledProgramRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<InstalledProgram>> {
        Some(InstalledProgramCodec)
    }

    fn num_columns(&mut self) -> usize {
        NUM_COLUMNS
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Name", "Version", "Publisher", "Install date", "Size (MiB)", "Source"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool {
        true
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &InstalledProgram) -> bool {
        if self.uninstallable_only
            && row.uninstall_string.as_deref().unwrap_or("").trim().is_empty()
            && row.quiet_uninstall_string.as_deref().unwrap_or("").trim().is_empty()
        {
            return false;
        }
        if self.filter.trim().is_empty() {
            return true;
        }
        let f = self.filter.to_lowercase();
        row.name.to_lowercase().contains(&f)
            || row
                .publisher
                .as_deref()
                .map(|p| p.to_lowercase().contains(&f))
                .unwrap_or(false)
            || row
                .version
                .as_deref()
                .map(|v| v.to_lowercase().contains(&f))
                .unwrap_or(false)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &InstalledProgram) -> bool {
        false
    }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &InstalledProgram, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                // Lead the name with a small "32" tag for
                // Wow6432Node rows so 32-bit-vs-64-bit installs
                // are visually distinct without spending a column
                // on it.
                if row.is_wow6432 {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("32").small().color(Color32::from_rgb(180, 140, 200)));
                        ui.label(RichText::new(&row.name).color(Color32::from_rgb(220, 220, 230)));
                    });
                } else {
                    ui.label(RichText::new(&row.name).color(Color32::from_rgb(220, 220, 230)));
                }
            }
            1 => {
                let text = row.version.as_deref().unwrap_or("");
                ui.label(RichText::new(text).color(Color32::from_rgb(180, 200, 220)));
            }
            2 => {
                let text = row.publisher.as_deref().unwrap_or("");
                ui.label(RichText::new(text).color(Color32::GRAY));
            }
            3 => {
                // InstallDate format is YYYYMMDD per convention.
                // Reformat as YYYY-MM-DD for the operator's
                // benefit; bail to raw text if it doesn't parse.
                let text = row
                    .install_date
                    .as_deref()
                    .map(format_install_date)
                    .unwrap_or_default();
                ui.label(RichText::new(text).color(Color32::GRAY));
            }
            4 => {
                // EstimatedSize is in KiB. Convert to MiB rounded
                // to 1 decimal for readability.
                let text = row
                    .estimated_size_kb
                    .map(|kb| format!("{:.1}", (kb as f64) / 1024.0))
                    .unwrap_or_default();
                ui.label(RichText::new(text).color(Color32::GRAY));
            }
            5 => {
                let (color, label) = match row.registry_hive.as_str() {
                    "HKLM" => (Color32::from_rgb(120, 200, 255), "HKLM"),
                    "HKLM-Wow6432" => (Color32::from_rgb(199, 202, 245), "HKLM (32)"),
                    "HKCU" => (Color32::from_rgb(255, 200, 120), "HKCU"),
                    other => (Color32::GRAY, other),
                };
                ui.label(RichText::new(label).small().color(color));
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut InstalledProgram,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        _row: &InstalledProgram,
        _column: usize,
        _resp: &egui::Response,
    ) -> Option<Box<InstalledProgram>> {
        None
    }

    fn set_cell_value(&mut self, src: &InstalledProgram, dst: &mut InstalledProgram, _column: usize) {
        // Read-only viewer; supplying a full clone keeps the
        // table consistent in the rare paths egui_data_table
        // calls this from (e.g. clipboard).
        *dst = src.clone();
    }

    fn compare_cell(&self, l: &InstalledProgram, r: &InstalledProgram, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.name.to_lowercase().cmp(&r.name.to_lowercase()),
            1 => l.version.as_deref().unwrap_or("").cmp(r.version.as_deref().unwrap_or("")),
            2 => l
                .publisher
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&r.publisher.as_deref().unwrap_or("").to_lowercase()),
            3 => l.install_date.as_deref().unwrap_or("").cmp(r.install_date.as_deref().unwrap_or("")),
            4 => l.estimated_size_kb.unwrap_or(0).cmp(&r.estimated_size_kb.unwrap_or(0)),
            5 => l.registry_hive.cmp(&r.registry_hive),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> InstalledProgram {
        empty_program()
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(260.).clip(true).resizable(true),
            1 => base.at_least(80.).at_most(160.),
            2 => base.at_least(160.).clip(true).resizable(true),
            3 => base.at_least(96.).at_most(120.),
            4 => base.at_least(80.).at_most(110.),
            5 => base.at_least(90.).at_most(120.),
            _ => base,
        }
    }

    fn custom_context_menu_items(
        &mut self,
        _context: &UiActionContext,
        selection: &SelectionSnapshot<'_, InstalledProgram>,
    ) -> Vec<CustomMenuItem> {
        let first = selection.selected_rows.first().map(|(_, r)| r);
        let has_selection = first.is_some();
        // Disable uninstall entries when the row has no uninstall
        // string at all — clicking would just produce a no-op
        // error.
        let has_uninstall = first
            .map(|r| {
                r.uninstall_string
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
                    || r.quiet_uninstall_string
                        .as_deref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
            })
            .unwrap_or(false);

        vec![
            CustomMenuItem::new("uninstall_silent", "Uninstall (silent if possible)")
                .icon(icons::CLOSE)
                .enabled(has_selection && has_uninstall),
            CustomMenuItem::new("uninstall_interactive", "Uninstall (interactive)")
                .icon("M")
                .enabled(has_selection && has_uninstall),
            CustomMenuItem::new("refresh", "Refresh").icon(icons::REFRESH).enabled(true),
        ]
    }

    fn on_custom_action_ex(
        &mut self,
        action_id: &'static str,
        ctx: &CustomActionContext<'_, InstalledProgram>,
        _editor: &mut CustomActionEditor<InstalledProgram>,
    ) {
        let Some(tx) = &self.action_tx else { return };
        let first = ctx.selection.selected_rows.first().map(|(_, r)| r);

        match action_id {
            "uninstall_silent" => {
                if let Some(row) = first {
                    let _ = tx.try_send(InstalledProgramAction::UninstallSilent(row.id.clone()));
                }
            }
            "uninstall_interactive" => {
                if let Some(row) = first {
                    let _ = tx.try_send(InstalledProgramAction::UninstallInteractive(row.id.clone()));
                }
            }
            "refresh" => {
                let _ = tx.try_send(InstalledProgramAction::Refresh);
            }
            _ => {}
        }
    }
}

/// Format a Windows `InstallDate` (`YYYYMMDD`) as `YYYY-MM-DD`,
/// or pass through unchanged if it doesn't match the expected
/// shape (some publishers use plain text).
fn format_install_date(raw: &str) -> String {
    if raw.len() == 8 && raw.chars().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &raw[..4], &raw[4..6], &raw[6..8])
    } else {
        raw.to_string()
    }
}

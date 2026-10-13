use database::schema::User;
use eframe::egui::{CentralPanel, Frame, Ui, scroll_area};
use egui_data_table::{egui::Widget, Renderer};
use super::{row_viewer::DatabaseTableSelection, DatabaseEditor};
use crate::ui_tools::controls::{self, BarKind, Metrics};
use crate::ui_tools::icons::svg;
use crate::ui_tools::theme;

/// Tables the editor can load, in menu order.
const TABLES: [(DatabaseTableSelection, &str); 8] = [
    (DatabaseTableSelection::Task, "Tasks"),
    (DatabaseTableSelection::Service, "Services"),
    (DatabaseTableSelection::Customer, "Customers"),
    (DatabaseTableSelection::Computer, "Computers"),
    (DatabaseTableSelection::User, "Users"),
    (DatabaseTableSelection::DiagSession, "Diagnostic Sessions"),
    (DatabaseTableSelection::DiagEntry, "Diagnostic Entries"),
    (DatabaseTableSelection::PluginReg, "Plugin Registry"),
];

impl DatabaseEditor {
    pub fn ui(&mut self, ui: &mut Ui, _current_user: Option<User>) {
        self.receive(ui.ctx());
        let m = Metrics::get(ui.ctx());
        eframe::egui::Panel::top("Database Editor Top Panel")
            .frame(Frame::NONE)
            .exact_size(m.bar_h)
            .show(ui, |ui|
        {
            controls::bar(ui, BarKind::Tool, |ui| {
                controls::search_field(ui, &mut self.database_viewer.filter, "Search", 180.);

                let selected = &mut self.database_viewer.selected_table;
                let current_selection = selected.clone();

                ui.add(svg::TABLE.image(m.icon_md).tint(theme::accent_secondary(ui)));
                controls::dropdown(ui, "table selection", selected, &TABLES, 180.);

                if current_selection != *selected {
                    let _ = self.data_selection_tx.try_send(selected.clone());
                }

                if controls::icon_text_button(ui, svg::DATABASE, "Get Data").clicked() {
                    self.start_idx = 0;
                    let _ = self.data_selection_tx.try_send(self.database_viewer.selected_table.clone());
                }

                if controls::icon_text_button(ui, svg::PLUS_CIRCLE, "Load +200").clicked() {
                    self.start_idx += 200;
                    let _ = self.data_selection_tx.try_send(self.database_viewer.selected_table.clone());
                }

            });
        });

        CentralPanel::default()
            .show(ui, |ui| 
        {
            if let Some(table) = self.table_map.get_mut(&self.database_viewer.selected_table.as_str().to_string()) {
                Renderer::new(table, &mut self.database_viewer)
                    // .with_table_row_height(80.)
                    .with_style_modify(|s| {
                        s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                        s.single_click_edit_mode = true;
                        s.auto_shrink = [false, false].into();
                    })
                    .ui(ui);
            } else {
                controls::empty_state(
                    ui,
                    svg::DATABASE,
                    "No table loaded",
                    "Pick a table and press Get Data.",
                    &[],
                );
            }
        });  
    }
}
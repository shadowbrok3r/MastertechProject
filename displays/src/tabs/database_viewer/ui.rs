use database::schema::User;
use eframe::egui::{CentralPanel, ComboBox, TextEdit, Ui, scroll_area};
use egui_data_table::{egui::Widget, Renderer};
use super::{row_viewer::DatabaseTableSelection, DatabaseEditor};

impl DatabaseEditor {
    pub fn ui(&mut self, ui: &mut Ui, _current_user: Option<User>) {
        self.receive(ui.ctx());
        eframe::egui::Panel::top("Database Editor Top Panel")
            .exact_size(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                ui.add(TextEdit::singleline(&mut self.database_viewer.filter)
                    .desired_width(150.)
                    .hint_text(" Search"));
                
                let selected_text = self.database_viewer.selected_table.as_str().to_string();
                let selected = &mut self.database_viewer.selected_table;
                let current_selection = selected.clone();

                ui.add_space(5.);

                ComboBox::new("table selection", "")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            selected, 
                            DatabaseTableSelection::Task, 
                            "Tasks"
                        );
                        ui.selectable_value(
                            selected,
                            DatabaseTableSelection::Service,
                            "Services",
                        );
                        ui.selectable_value(
                            selected,
                            DatabaseTableSelection::Customer,
                            "Customers",
                        );
                        ui.selectable_value(
                            selected,
                            DatabaseTableSelection::Computer,
                            "Computers",
                        );
                        ui.selectable_value(
                            selected, 
                            DatabaseTableSelection::User, 
                            "Users"
                        );
                        ui.separator();
                        ui.selectable_value(
                            selected,
                            DatabaseTableSelection::DiagSession,
                            "Diagnostic Sessions",
                        );
                        ui.selectable_value(
                            selected,
                            DatabaseTableSelection::DiagEntry,
                            "Diagnostic Entries",
                        );
                        ui.selectable_value(
                            selected,
                            DatabaseTableSelection::PluginReg,
                            "Plugin Registry",
                        );
                    });

                if current_selection != *selected {
                    let _ = self.data_selection_tx.try_send(selected.clone());
                }

                ui.add_space(5.);

                if ui.button("Get Data").clicked() {
                    self.start_idx = 0;
                    let _ = self.data_selection_tx.try_send(self.database_viewer.selected_table.clone());
                }

                ui.add_space(5.);

                if ui.button("Load +200").clicked() {
                    self.start_idx += 200;
                    let _ = self.data_selection_tx.try_send(self.database_viewer.selected_table.clone());
                }

            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
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
            }
        });  
    }
}
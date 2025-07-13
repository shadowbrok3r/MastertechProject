use database::schema::User;
use eframe::egui::{CentralPanel, ComboBox, TextEdit, TopBottomPanel, Ui};
use egui_data_table::{egui::Widget, Renderer};
use super::{row_viewer::DatabaseTableSelection, DatabaseEditor};

impl DatabaseEditor {
    pub fn ui(&mut self, ui: &mut Ui, _current_user: Option<User>) {
        self.receive();
        TopBottomPanel::top("Database Editor Top Panel")
            .exact_height(30.)
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
                        // ui.selectable_value(
                        //     &mut selected,
                        //     DatabaseTable::TaskNote,
                        //     "Task Notes",
                        // );
                        // ui.selectable_value(
                        //     &mut selected,
                        //     DatabaseTable::ConnectedClient,
                        //     "Connected Clients",
                        // );
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
                        s.single_click_edit_mode = true;
                        s.auto_shrink = [false, false].into();
                        
                    })
                    .ui(ui);
            }
        });  
    }
}
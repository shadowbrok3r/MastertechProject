use database::schema::User;
use eframe::egui::{Button, CentralPanel, CollapsingHeader, ComboBox, Id, Layout, RichText, ScrollArea, Separator, SidePanel, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use super::{row_viewer::DatabaseTable, DatabaseViewer};

impl DatabaseViewer {
    pub fn show(&mut self, ui: &mut Ui, current_user: Option<User>) {
        TopBottomPanel::top("Database Viewer Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.database_viewer.filter)
                    .hint_text(" Search")
                    .ui(ui);
            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            ui.horizontal(|ui| {
                ui.add_space(10.);

                let selected_text = self.database_viewer.selected.as_str().to_string();
                let selected = self.database_viewer.selected;
                let current_selection = selected.as_str().clone();

                ComboBox::new("table selection", "")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut selected, 
                            DatabaseTable::Task, 
                            "Tasks"
                        );
                        ui.selectable_value(
                            &mut selected,
                            DatabaseTable::Customer,
                            "Customers",
                        );
                        ui.selectable_value(
                            &mut selected,
                            DatabaseTable::Ticket,
                            "Services",
                        );
                        ui.selectable_value(
                            &mut selected,
                            DatabaseTable::Computer,
                            "Computers",
                        );
                        ui.selectable_value(
                            &mut selected,
                            DatabaseTable::TaskNote,
                            "Task Notes",
                        );
                        ui.selectable_value(
                            &mut selected,
                            DatabaseTable::ConnectedClient,
                            "Connected Clients",
                        );
                    });

                if current_selection != *selected {
                    let selection = selected.as_str();
                }
            });
            ui.add_space(5.);

            if let Some(table) = self.data_viewer.get_mut(&self.selected.as_str().to_string()) {
                // style.single_click_edit_mode = true;
                Renderer::new(table, &mut self.database_viewer)
                    .with_style(egui_data_table::Style::default())
                    .ui(ui);
            }
        });  
    }
}
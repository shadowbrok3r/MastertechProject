use crate::app_state::MtechServerContext;
use database::schema::Store;
use displays::egui_data_table::{
    viewer::{default_hotkeys, UiActionContext},
    Renderer, RowViewer, UiAction,
};
use eframe::egui::{CentralPanel, ComboBox, KeyboardShortcut, SidePanel, Ui};

use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

impl MtechServerContext {
    pub fn task_table_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys")
            .default_width(500.)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.heading("Hotkeys");
                    ui.separator();
                    ui.add_space(0.);

                    // for (k, a) in &self.data_viewer.hotkeys {
                    //     Button::new(format!("{a:?}"))
                    //         .shortcut_text(ui.ctx().format_shortcut(k))
                    //         .ui(ui);
                    //     ui.add_space(10.);
                    // }
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                // TextEdit::singleline(&mut self.data_viewer.filter).ui(ui);

                ui.add_space(10.);

                let selected = &mut self.store_selection;
                let selected_text = match selected {
                    76 => Store::RIV.as_str(),
                    73 => Store::LTN.as_str(),
                    74 => Store::MUR.as_str(),
                    78 => Store::WJ.as_str(),
                    75 => Store::ORE.as_str(),
                    72 => Store::AF.as_str(),
                    77 => Store::SAN.as_str(),
                    _ => Store::RIV.as_str(),
                };

                let current_selection = selected.clone();

                ComboBox::new("Store_Selection", "")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(selected, 76, "RIV");
                        ui.selectable_value(selected, 73, "LTN");
                        ui.selectable_value(selected, 74, "MUR");
                        ui.selectable_value(selected, 78, "WJ");
                        ui.selectable_value(selected, 75, "ORE");
                        ui.selectable_value(selected, 72, "AF");
                        ui.selectable_value(selected, 77, "SAN");
                    })
                    .response;

                if current_selection != *selected {
                    spawn_local(async move {});
                }
            });

            ui.add(Renderer::new(&mut self.data_table, &mut self.data_viewer));
        });
    }
}

// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone)]
pub struct MyRowData(pub String, pub String, pub String, pub String, pub bool);

/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct TaskRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
}

impl RowViewer<MyRowData> for TaskRowViewer {
    fn num_columns(&mut self) -> usize {
        5
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Item Code", "Serial Number", "Attached", "Location", ""][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &MyRowData) -> bool {
        row.0.contains(&self.filter) || row.1.contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &MyRowData, column: usize) {
        todo!()
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut MyRowData,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        todo!()
    }

    fn set_cell_value(&mut self, src: &MyRowData, dst: &mut MyRowData, column: usize) {
        todo!()
    }

    fn new_empty_row(&mut self) -> MyRowData {
        todo!()
    }
}

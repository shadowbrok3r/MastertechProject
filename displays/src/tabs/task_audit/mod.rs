use crate::{egui_data_table::{viewer::{default_hotkeys, UiActionContext}, Renderer, RowViewer, UiAction}, Spawner};
use eframe::egui::{Button, CentralPanel, Color32, ComboBox, KeyboardShortcut, RichText, SidePanel, TextEdit, TopBottomPanel, Ui, Widget};
use database::schema::{helper_traits::{EmployeeHelper, UserHelper}, prestashop_schema::Employee};
use crate::{app_state::SharedContext, PlatformSpawner};
use serde::Serialize;

impl SharedContext {
    pub fn task_table_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys-TaskAudit")
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

        TopBottomPanel::top("Task Audit Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    TextEdit::singleline(&mut self.serials_viewer.filter)
                        .hint_text("Search for SO# / Customer")
                        .ui(ui);

                    ui.add_space(10.);

                    if Button::new("Refresh").ui(ui).clicked() {
                        let order_tx = self.presta_order_channel.0.clone();
                        let usr = self.current_user.clone().unwrap_or_default();
                        let id = usr.id_prestashop.unwrap_or_default();
                        let mut employee = Employee::default();
                        employee.id = format!("{id}");
                        employee.id_store = usr.id_store.unwrap_or_default();
                        PlatformSpawner::spawn(async move {
                            let services = employee.get_my_services().await;
                            match services {
                                Ok(svcs) => order_tx.try_send(svcs).unwrap(),
                                Err(e) => log::info!("Error getting my services: {e:?}"),
                            }
                        });
                    }
                    ui.add_space(10.);
                    
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                // TextEdit::singleline(&mut self.data_viewer.filter).ui(ui);

                ui.add_space(10.);

                let selected = &mut self.store_selection;

                let mut usr = self.current_user.clone().unwrap_or_default();
                let selected_text = usr.get_store_from_odoo_id().unwrap_or_default();

                let current_selection = selected.clone();

                ComboBox::new("Store_Selection", "")
                    .selected_text(selected_text.as_str())
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
                    PlatformSpawner::spawn(async move {});
                }
            });

            ui.add(Renderer::new(&mut self.my_orders_table, &mut self.my_orders_viewer));
        });
    }
}



// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone)]
pub struct PrestashopOrderData(pub String, pub String, pub String, pub String, pub String);

/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct TaskRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
}

impl RowViewer<PrestashopOrderData> for TaskRowViewer {
    fn num_columns(&mut self) -> usize {
        5
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Needs Call"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &PrestashopOrderData) -> bool {
        row.0.contains(&self.filter) || row.1.contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, _ui: &mut eframe::egui::Ui, _row: &PrestashopOrderData, _column: usize) {
        todo!()
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut PrestashopOrderData,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        let res = match column {
            0 => ui.label(row.0.clone()),
            1 => ui.label(row.1.clone()),
            2 => ui.label(row.2.clone()),
            3 => ui.label(row.3.clone()),
            4 => ui.label(row.4.clone()),
            _ => unreachable!(),
        };
        Some(res)
    }

    fn set_cell_value(
        &mut self,
        src: &PrestashopOrderData,
        dst: &mut PrestashopOrderData,
        column: usize,
    ) {
        match column {
            0 => dst.0 = src.0.clone(),
            1 => dst.1 = src.1.clone(),
            2 => dst.2 = src.2.clone(),
            3 => dst.3 = src.3.clone(),
            4 => dst.4 = src.4.clone(),
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &PrestashopOrderData,
        row_r: &PrestashopOrderData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l.1.cmp(&row_r.1),
            2 => row_l.2.cmp(&row_r.2),
            3 => row_l.3.cmp(&row_r.3),
            4 => row_l.4.cmp(&row_r.4),
            _ => row_l.0.cmp(&row_r.0)
        }
    }

    fn new_empty_row(&mut self) -> PrestashopOrderData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        PrestashopOrderData(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
    }
}

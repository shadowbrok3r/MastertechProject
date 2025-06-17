use eframe::egui::{Button, CentralPanel, Color32, ComboBox, ScrollArea, SidePanel, Spinner, TextEdit, TopBottomPanel, Ui, Widget};
use egui_data_table::Renderer;
use crate::app_state::SharedContext;
use crate::{PlatformSpawner, Spawner};
use database::schema::Store;
use log::info;

pub mod row_viewer;
pub mod stock_operations;

pub use row_viewer::*;
pub use stock_operations::*;

impl SharedContext {
    pub fn stock_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys")
            .default_width(300.)
            .show_animated_inside(ui, self.serials_viewer.show_hotkeys, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.heading("Hotkeys");
                    ui.separator();
                    ui.add_space(0.);
                    ScrollArea::new([false, true]).show(ui, |ui| {
                        for (k, a) in &self.serials_viewer.hotkeys {
                            Button::new(format!("{a:?}"))
                                .shortcut_text(ui.ctx().format_shortcut(k))
                                .ui(ui);
                            ui.add_space(10.);
                        }
                    });
                });
            });

        TopBottomPanel::top("StockTopPanel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    TextEdit::singleline(&mut self.serials_viewer.filter)
                        .hint_text("Search for Item Code or S/N")
                        .ui(ui);

                    ui.add_space(10.);

                    let selected = &mut self.store_selection;
                    let current = selected.clone();

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
                        });

                    if *selected != current {
                        let stock_tx = self.stock_channel.0.clone();
                        let store_selection = self.store_selection;
                        PlatformSpawner::spawn(async move {
                            info!("Store: {:?}", store_selection);
                            let stock = get_stock(stock_tx.clone(), store_selection).await;
                            info!("Stock call: {stock:?}");
                        });
                    }
                    ui.add_space(10.);

                    if Button::new("Refresh").ui(ui).clicked() {
                        let stock_tx = self.stock_channel.0.clone();
                        let store_selection = self.store_selection;
                        PlatformSpawner::spawn(async move {
                            let stock = get_stock(stock_tx.clone(), store_selection).await;
                            info!("Stock call: {stock:?}");
                        });
                    }
                    ui.add_space(10.);

                    if Button::new("Refresh S/N Info").ui(ui).clicked() {
                        let tx = self.serial_channel.0.clone();
                        let data_table = self.serials_table.iter();
                        let sns = data_table.map(|r| r.1.clone()).collect::<Vec<String>>();
                        PlatformSpawner::spawn(async move {
                            let _res = find_attached_serials(sns, tx.clone()).await;
                        });
                    }
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            if self.serials_table.len() < 1 {
                ui.vertical_centered(|ui| {
                    ui.label("Pulling Store Stock Information..");
                    Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                });
            } else {
                Renderer::new(
                    &mut self.serials_table, 
                    &mut self.serials_viewer
                ).with_style_modify(|s| {
                    s.single_click_edit_mode = true;
                    s.auto_shrink = [false, false].into();
                }).ui(ui);
            }
        });
    }
}

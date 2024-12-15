use eframe::egui::{Button, CentralPanel, Color32, ScrollArea, SidePanel, Spinner, TextEdit, TopBottomPanel, Ui, Widget};
use crate::{app_state::SharedContext, tabs::stock::get_extra_stock_info};
use crate::{PlatformSpawner, Spawner};
use crate::egui_data_table::Renderer;
use log::info;

pub mod row_viewer;
pub use row_viewer::*;

impl SharedContext {
    pub fn stock_quantities_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys")
            .default_width(500.)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.heading("Hotkeys");
                    ui.separator();
                    ui.add_space(0.);
                    ScrollArea::new([false, true]).show(ui, |ui| {
                        for (k, a) in &self.stock_quantity_viewer.hotkeys {
                            Button::new(format!("{a:?}"))
                                .shortcut_text(ui.ctx().format_shortcut(k))
                                .ui(ui);
                            ui.add_space(10.);
                        }
                    });
                });
            });
        TopBottomPanel::top("StockTopPanel-Quantities")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    TextEdit::singleline(&mut self.stock_quantity_viewer.filter)
                        .hint_text("Search for Item Code")
                        .ui(ui);

                    ui.add_space(10.);

                    if Button::new("Refresh").ui(ui).clicked() {
                        let stock_tx = self.extra_stock_channel.0.clone();
                        PlatformSpawner::spawn(async move {
                            let stock = get_extra_stock_info(stock_tx.clone()).await;
                            info!("Stock call: {stock:?}");
                        });
                    }
                    ui.add_space(10.);
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            if self.stock_quantity_table.len() < 1 {
                ui.vertical_centered(|ui| {
                    ui.label("Pulling Company Stock Information..");
                    Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                });
            } else {   
                ui.add(Renderer::new(
                    &mut self.stock_quantity_table,
                    &mut self.stock_quantity_viewer,
                ));
            }
        });
    }
}

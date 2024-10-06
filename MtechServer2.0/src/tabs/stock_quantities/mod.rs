use crate::app_state::MtechServerContext;
use database::schema::Store;
use displays::egui_data_table::Renderer;
use eframe::egui::{
    Button, CentralPanel, ComboBox, ScrollArea, SidePanel, TextEdit, TopBottomPanel, Ui, Widget,
};

use log::info;
use wasm_bindgen_futures::spawn_local;

pub mod row_viewer;
pub mod stock_operations;

pub use row_viewer::*;
pub use stock_operations::*;

impl MtechServerContext {
    pub fn stock_quantities_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys-Quantities")
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
                    TextEdit::singleline(&mut self.data_viewer.filter)
                        .hint_text("Search for Item Code")
                        .ui(ui);

                    ui.add_space(10.);

                    if Button::new("Refresh").ui(ui).clicked() {
                        let stock_tx = self.stock_channel.0.clone();
                        let store_selection = self.store_selection;
                        spawn_local(async move {
                            let stock = get_stock(stock_tx.clone(), store_selection).await;
                            info!("Stock call: {stock:?}");
                        });
                    }
                    ui.add_space(10.);
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.add(Renderer::new(
                &mut self.stock_quantity_table,
                &mut self.stock_quantity_viewer,
            ));
        });
    }
}

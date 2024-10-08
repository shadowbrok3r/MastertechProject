use crate::{app_state::MastertechContext, tabs::stock::get_extra_stock_info};
use displays::egui_data_table::Renderer;
use eframe::egui::{
    Button, CentralPanel, ScrollArea, SidePanel, TextEdit, TopBottomPanel, Ui, Widget,
};

use log::info;

pub mod row_viewer;
pub use row_viewer::*;
use tokio::spawn;

impl MastertechContext {
    pub fn stock_quantities_viewer(&mut self, ui: &mut Ui) {
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
                        spawn(async move {
                            let stock = get_extra_stock_info(stock_tx.clone()).await;
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

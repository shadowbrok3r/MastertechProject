use crate::app_state::MtechServerContext;
use egui_extras::{Column, TableBuilder};
use eframe::egui::{Align, Layout, Ui};

impl MtechServerContext{
    pub fn customer_view(&mut self, ui: &mut Ui){ 
        if let Some(users) = self.store_users.as_ref(){
            let page = "Customers";

            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(100.0).range(50.0..=280.0).clip(true))
                .column(Column::remainder())
                .min_scrolled_height(0.0);

            table
            .header(20.0, |mut header|{
                header.col(|ui| {
                    ui.strong("Customer Name");
                });
                header.col(|ui| {
                    ui.strong("Phone#");
                });
                header.col(|ui| {
                    ui.strong("Last Service");
                });
                header.col(|ui| {
                    ui.strong("Non Completed Services");
                });
                header.col(|ui| {
                    ui.strong("Computers");
                });
            })
            .body(|mut body| {
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("Customer name");
                    });
                    row.col(|ui|{
                        ui.label("Phone");
                    });
                    row.col(|ui|{
                        ui.label("Last Service");
                    });
                    row.col(|ui|{
                        ui.label("Services");
                    });
                    row.col(|ui|{
                        ui.label("Computers");
                    });
                });
                body.row(20.0, |mut row| {

                });                
            });
        }
    }
}
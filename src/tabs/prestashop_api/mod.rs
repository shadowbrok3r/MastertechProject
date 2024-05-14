use eframe::egui::{Align, Button, Grid, Layout, RichText, Ui};
use log::info;

use crate::app_state::MastertechContext;

use self::resources::Resources;
pub mod api;
pub mod resources;

impl MastertechContext {
    pub fn presta_api(&mut self, ui: &mut Ui){ 
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.vertical(|ui|{ui.add_space(8.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});

        Grid::new("api_calls").min_col_width(self.widget_size).num_columns(1).min_row_height(8.0).spacing([10.0, 8.0]).show(
            ui, |ui| 
        {
            ui  
                .with_layout(Layout::top_down_justified(Align::Center),|ui|
            {

                let button = Button::new(RichText::new("Get").small().size(12.0));

                if ui.add(button).clicked(){
                    let tx = self.prestashop_api_tx.clone();
                    tokio::spawn(async move {
                        let api_call = self::api::Prestashop::default();
                        let tx1 = tx.clone();
                        let tx2 = tx.clone();
                        let tx3 = tx.clone();

                        // match tx.try_send(api_call.request_resource("addresses".to_string()).await.unwrap()){
                        //     Ok(_) => drop(tx),
                        //     Err(err) => info!("Error: {err:?}"),
                        // }

                        match tx1.try_send(api_call.request_resource("orders".to_string(), None).await.unwrap()){
                            Ok(_) => drop(tx1),
                            Err(err) => info!("Error: {err:?}"),
                        }

                        // match tx2.clone().try_send(api_call.request_resource("customers".to_string()).await.unwrap()){
                        //     Ok(_) => drop(tx2),
                        //     Err(err) => info!("Error: {err:?}"),
                        // }

                        // match tx3.clone().try_send(api_call.request_resource("employees".to_string()).await.unwrap()){
                        //     Ok(_) => drop(tx3),
                        //     Err(err) => info!("Error: {err:?}"),
                        // }
                    });
                }
                ui.end_row();

            });
        });
    }
}
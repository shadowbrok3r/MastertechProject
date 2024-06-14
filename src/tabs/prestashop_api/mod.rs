use std::collections::HashMap;

use eframe::egui::{Align, Button, Grid, Layout, RichText, Ui};
use log::info;
use resources::{Employee, Order};

use crate::app_state::MastertechContext;

use self::{api::PrestashopData, resources::Employees};
pub mod api;
pub mod resources;
pub mod deserializer;

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

                        // let employees: Vec<Employee> = api_call.request_resource("employees".to_string(), "employee".to_string(), Some("id".to_string())).await.unwrap();
                        // let employees: Vec<Employee> = api_call.request_resource_link("employees".to_string(), "employee".to_string(), Some("id".to_string())).await.unwrap();
                        // let orders: Vec<Order> = api_call.request_resource_link("orders".to_string(), "order".to_string(), Some("id".to_string())).await.unwrap();
                        // let x: Order = api_call.request_subresources_by_id("orders".to_string(), "order".to_string(),&3).await.unwrap();
                        let mut query = HashMap::new();
                        query.insert("filter[id_employee]", "48");

                        let y: Vec<Order> = api_call.request_resource_test("orders", query).await.unwrap();
                        // let orders_1: Vec<Order> = api_call.request_resource("orders".to_string(),"order".to_string(),  Some("id_customer".to_string())).await.unwrap();
                        // println!("employees: {:?}", employees);
                        // match tx.try_send(PrestashopData::Orders(orders)){
                        //     Ok(_) => drop(tx),
                        //     Err(err) => info!("Error: {err:?}"),
                        // }
                        
                        // match tx.try_send(PrestashopData::Employees(employees)){
                        //     Ok(_) => drop(tx),
                        //     Err(err) => info!("Error: {err:?}"),
                        // }
                    });
                }
                ui.end_row();

            });
        });
    }
}
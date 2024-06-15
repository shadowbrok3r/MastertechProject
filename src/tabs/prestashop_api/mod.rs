use eframe::egui::{Align, Button, Grid, Layout, RichText, Ui};
use log::info;
use crate::{app_state::MastertechContext, database::{prestashop_schema::{Address, Customer, Employee, Order}, schema::{CustomerData, PrestashopPayload}}};

pub mod api;
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
                let input = self.so_number.clone().parse::<i32>().unwrap_or(0);
                if ui.add(button).clicked(){
                    let tx = self.prestashop_api_tx.clone();
                    tokio::spawn(async move {
                        let api_call = self::api::Prestashop::default();
                        // let mut query = HashMap::new();
                        // query.insert("filter[id_employee]", "48");
                        // let employees: Vec<Employee> = api_call.request_resources(
                        //     "employees", 
                        //     "employee", 
                        //     Some("id"), 
                        //     query.clone()
                        // ).await.unwrap();

                        let order: Order = api_call.request_subresources_by_id(
                            "orders", 
                            "order", 
                            &input
                        ).await.unwrap();
                        
                        info!("order: {order:#?}");

                        let employee: Option<Employee> = if order.id_employee_split_rep != 0{
                            let employee: Employee = api_call.request_subresources_by_id(
                                "employees", 
                                "employee", 
                                &order.id_employee_sales_rep
                            ).await.unwrap();

                            info!("employee: {employee:#?}");

                            let _employee_2: Employee = api_call.request_subresources_by_id(
                                "employees", 
                                "employee", 
                                &order.id_employee_split_rep
                            ).await.unwrap();

                            Some(employee)
                        }else{
                            None
                        };

                        let customer: Customer = api_call.request_subresources_by_id(
                            "customers", 
                            "customer", 
                            &order.id_customer.id
                        ).await.unwrap();

                        info!("customer: {customer:#?}");

                        let address: Address = api_call.request_subresources_by_id(
                            "addresses", 
                            "address", 
                            &order.id_address_delivery.id
                        ).await.unwrap();

                        info!("address: {address:#?}");

                        let customer = CustomerData{
                            cust_code: address.id_customer.id,
                            name: format!("{} {}", &address.firstname, &address.lastname),
                            phone_number: address.phone.clone().to_string(),
                            // phone_number_2: address.phone_mobile.clone().unwrap_or(0).to_string(),
                            email: customer.email,
                            ..Default::default()
                        };

                        let presta_payload = PrestashopPayload {
                            customer,
                            order,
                            employee,
                            address,
                        };

                        match tx.try_send(presta_payload){
                            Ok(_) => drop(tx),
                            Err(err) => info!("Error: {err:?}"),
                        };
                    });
                }
                ui.end_row();

            });
        });
    }
}
use crate::app_state::MtechServerContext;
use database::schema::TicketData;
use egui_extras::{Column, TableBuilder};
use eframe::egui::{Align, Color32, FontId, Layout, Ui};

impl MtechServerContext{
    pub fn customer_view(&mut self, ui: &mut Ui){ 
        if let Some(users) = self.store_users.as_ref(){
            ui.horizontal(|ui| ui.add_space(ui.available_width() / 3.0));
            ui.vertical_centered_justified( |ui| {

            
                ui.style_mut().override_font_id = Some(FontId::proportional(15.0));
                let customers = &self.data_output.customers;
                let computers = &self.data_output.computers;
                let services = &self.data_output.tickets;

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(Layout::top_down_justified(Align::Center))
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::initial(90.0))
                    .column(Column::initial(300.0).range(50.0..=280.0).clip(true))
                    .column(Column::initial(150.0))
                    .column(Column::initial(100.0))
                    .column(Column::exact(30.0))
                    .column(Column::exact(30.0))
                    .min_scrolled_height(0.0);

                table.header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.colored_label(Color32::LIGHT_RED,"ID");
                    });
                    header.col(|ui| {
                        ui.colored_label(Color32::LIGHT_RED,"Customer Name");
                    });
                    header.col(|ui| {
                        ui.colored_label(Color32::LIGHT_RED,"Phone#");
                    });
                    header.col(|ui| {
                        ui.colored_label(Color32::LIGHT_RED,"Last Service");
                    });
                    header.col(|ui| {
                        ui.colored_label(Color32::LIGHT_RED,"Non Completed Services");
                    });
                    header.col(|ui| {
                        ui.colored_label(Color32::LIGHT_RED,"Computers");
                    });
                })
                .body(| body| {
                    body.rows(20.0, customers.len(), |mut row| {
                        let idx = row.index();
                        if let Some(customer) = customers.get(idx){

                            let svcs: Vec<TicketData> = services.iter().filter(|svc| {
                                if let (Some(cust_id), Some(c_id)) = (svc.customer.clone(), customer.id.clone()){   
                                    cust_id.0.id == c_id.0.id
                                } else { false }
                            }).cloned().collect();

                            // self.tasks.iter().filter(|task| task.completed)
                            // let computers: Vec<ComputerData> = self.tasks.iter().filter(|computer| {
                            //     if let Some(ticket) = computer.service_ticket{
                            //         if let Some(computer) = ticket.computer{
                            //             computer.id.unwrap() == ticket.
                            //         }
                            //     }
                            // });

                            row.col(|ui|{
                                ui.add_space(5.0);
                                ui.colored_label(Color32::LIGHT_RED, customer.cust_code.clone());
                            });
                            row.col(|ui|{
                                ui.label(customer.name.clone().trim());
                            });
                            row.col(|ui|{
                                ui.vertical_centered(|ui | {
                                    ui.label(customer.phone_number.clone());
                                });
                            });
                            row.col(|ui|{
                                ui.label(customer.li_doc.clone());
                            });
                            row.col(|ui|{
                                ui.label(format!("{}", svcs.len()));
                            });
                            row.col(|ui|{
                                ui.label(format!("{}", computers.len()));
                            });
                        }
                    });      
                });


                // let table = TableBuilder::new(ui)
                //     .striped(true)
                //     .cell_layout(Layout::top_down_justified(Align::Center))
                //     .cell_layout(Layout::left_to_right(Align::Center))
                //     .cell_layout(Layout::left_to_right(Align::Center))
                //     .column(Column::initial(90.0))
                //     .column(Column::initial(300.0).range(50.0..=280.0).clip(true))
                //     .column(Column::initial(150.0))
                //     .column(Column::initial(100.0))
                //     .column(Column::exact(30.0))
                //     .column(Column::exact(30.0))
                //     .min_scrolled_height(0.0);

                // table.header(20.0, |mut header| {
                //     header.col(|ui| {
                //         ui.colored_label(Color32::LIGHT_RED,"ID");
                //     });
                //     header.col(|ui| {
                //         ui.colored_label(Color32::LIGHT_RED,"Customer Name");
                //     });
                //     header.col(|ui| {
                //         ui.colored_label(Color32::LIGHT_RED,"Phone#");
                //     });
                //     header.col(|ui| {
                //         ui.colored_label(Color32::LIGHT_RED,"Last Service");
                //     });
                //     header.col(|ui| {
                //         ui.colored_label(Color32::LIGHT_RED,"Non Completed Services");
                //     });
                //     header.col(|ui| {
                //         ui.colored_label(Color32::LIGHT_RED,"Computers");
                //     });
                // })
                // .body(| body| {
                //     body.rows(20.0, customers.len(), |mut row| {
                //         let idx = row.index();
                //         if let Some(svc) = services.get(idx){

                //             // self.tasks.iter().filter(|task| task.completed)
                //             // let computers: Vec<ComputerData> = self.tasks.iter().filter(|computer| {
                //             //     if let Some(ticket) = computer.service_ticket{
                //             //         if let Some(computer) = ticket.computer{
                //             //             computer.id.unwrap() == ticket.
                //             //         }
                //             //     }
                //             // });

                //             row.col(|ui|{
                //                 ui.add_space(5.0);
                //                 ui.colored_label(Color32::LIGHT_RED, svc..cust_code.clone());
                //             });
                //             row.col(|ui|{
                //                 ui.label(customer.name.clone().trim());
                //             });
                //             row.col(|ui|{
                //                 ui.vertical_centered(|ui | {
                //                     ui.label(customer.phone_number.clone());
                //                 });
                //             });
                //             row.col(|ui|{
                //                 ui.label(customer.li_doc.clone());
                //             });
                //             row.col(|ui|{
                //                 ui.label(format!("{}", svcs.len()));
                //             });
                //             row.col(|ui|{
                //                 ui.label(format!("{}", computers.len()));
                //             });
                //         }
                //     });      
                // });
            });
        }
    }
}
use eframe::egui::{Button, Color32, FontId, Grid, Hyperlink, Margin, RichText, ScrollArea, TextEdit, Ui, Vec2, Widget};
use database::{schema::{CustomerData, LiveTaskPayload, Record, TicketData, User}, DATABASE};
use crate::{tabs::task_audit::row_viewer::BASE_URL, Interaction, PlatformSpawner, Spawner};
use chrono::{DateTime, Utc};

use super::return_colors;

pub fn display_ticket_page(
    ui: &mut Ui, 
    task: &mut LiveTaskPayload, 
    service_ticket: Option<&mut TicketData>,
    customer: Option<&mut CustomerData>,
    avail_size: Vec2, 
    store_users: &Vec<User>, 
    current_user: User,
) {
    ui.vertical_centered_justified(|ui| {
        ui.colored_label(Color32::LIGHT_GREEN, format!("ID: {}", task.id.key().to_string()));

        ui.add_space(10.0);

        let ticket = if let Some(ticket) = service_ticket { ticket }  else { &mut TicketData::default() };
        ui.group(|ui| {
            Grid::new("Task Modal - Task Info Page")
                .spacing(Vec2::new(2., 4.))
                .max_col_width(avail_size.x / 4.35)
                .min_col_width(avail_size.x / 4.35)
                .with_row_color(|num, style| return_colors(num, style))
                .num_columns(4)
                .show(ui, |ui| 
            {
                if current_user.is_admin() {
                    ui.colored_label(Color32::LIGHT_RED, "Service #");
                    ui.push_id(format!("Service #{:?}", task.service_number), |ui| {
                        ui.set_width(avail_size.x/5.);
                        task.interact_service_number(ui);
                    });
                } else {
                    ui.label("");
                    ui.label("");
                }

                if let Some(service_number) = task.service_number.as_ref() {
                    ui.colored_label(Color32::LIGHT_RED, "Prestashop Order");
                    Hyperlink::from_label_and_url(
                        service_number.clone(), 
                        format!("{BASE_URL}{}", service_number)
                    )
                    .open_in_new_tab(true)
                    .ui(ui);
                } else {
                    ui.label("");
                    ui.label("");
                }

                ui.end_row();

                ui.colored_label(Color32::LIGHT_RED, "Assignee");
                ui.push_id(format!("Assignee {}", task.assignee.key().to_string()), |ui| {
                    task.interact_assignee(ui, store_users, &current_user);
                });
                
                ui.colored_label(Color32::LIGHT_RED, "Status");
                ui.push_id(format!("Status {}", task.status.as_str()), |ui| {
                    task.interact_status(&current_user, ui);
                });
                
                ui.end_row();

                let customer = if let Some(customer) = customer { customer }  else { &mut CustomerData::default() };

                ui.colored_label(Color32::LIGHT_RED, "Technician");
                ui.label(&ticket.tech);
                ui.colored_label(Color32::LIGHT_RED, "Customer ID");
                ui.horizontal(|ui| {
                    TextEdit::singleline(&mut customer.cust_code).desired_width(50.).ui(ui);

                    if Button::new("Save").ui(ui).clicked() {
                        let id_customer = customer.cust_code.clone();
                        PlatformSpawner::spawn(async move {
                            let cust_data = CustomerData::find_customer_by_id(&id_customer).await;
                            log::info!("Cust Data {cust_data:?}");
                        });
                    }
                });


                ui.end_row();

                ui.colored_label(Color32::LIGHT_RED, "Salesman");
                ui.label(&ticket.salesman);
                ui.colored_label(Color32::LIGHT_RED, "Name");
                ui.label(&customer.name);
                ui.end_row();

                ui.colored_label(Color32::LIGHT_RED, "Split Rep");
                ui.label(&ticket.sales_rep);
                ui.colored_label(Color32::LIGHT_RED, "Phone#");
                ui.label(&customer.phone_number);
                ui.end_row();
                
                ui.colored_label(Color32::LIGHT_RED, "Terms");
                ui.label(&ticket.terms);
                ui.colored_label(Color32::LIGHT_RED, "Phone2");
                ui.label(&customer.phone_number_2);
                ui.end_row();

                ui.colored_label(Color32::LIGHT_RED, "Total on Order");
                ui.label(&ticket.ticket_total);
                ui.colored_label(Color32::LIGHT_RED, "Email");
                ui.label(&customer.email);
                ui.end_row();
                
                ui.colored_label(Color32::LIGHT_RED, "Tur Sent:");
                let date: DateTime<Utc> = ticket.created_at.clone().into();
                ui.label(date.date_naive().to_string());

                ui.colored_label(Color32::LIGHT_RED, "Due Date");
                ui.push_id(format!("Due Date {}", task.due_date), |ui| {
                    task.interact_due_date(ui);
                });
                ui.end_row();
            });
        });

        ui.add_space(15.);

        ui.group(|ui| {
            ScrollArea::vertical()
            .max_height(avail_size.y/1.3)
            .show(ui, |ui| 
            {
                Grid::new("Checkin Notes and Recommendations")
                .spacing(Vec2::new(2., 4.))
                .max_col_width(avail_size.x / 2.15)
                .min_col_width(avail_size.x / 2.15)
                .num_columns(2)
                .show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ui.label(
                            RichText::new("Checkin Notes:")
                                .font(FontId::proportional(15.0)),
                        );

                        ui.add_space(10.);
                        
                        TextEdit::multiline(&mut ticket.checkin_notes)
                        .margin(Margin::symmetric(10, 3))
                        .desired_rows(15)
                        .desired_width(ui.available_width())
                        .ui(ui);
                    });

                    ui.vertical_centered_justified(|ui| {
                        ui.label(
                            RichText::new("Recommendations:")
                                .font(FontId::proportional(15.0))
                        );

                        ui.add_space(10.);

                        let res = TextEdit::multiline(&mut task.task_description)
                            .margin(Margin::symmetric(10, 3))
                            .desired_rows(15)
                            .desired_width(ui.available_width())
                            .ui(ui);

                        if res.lost_focus() {
                            let task_description = task.task_description.clone();
                            let task_id = task.id.clone();
                            PlatformSpawner::spawn(async move {
                                // let _result = task.update_task_description().await;
                                match DATABASE
                                .query("UPDATE $id SET task_description=$description")
                                .bind(("id", task_id))
                                .bind(("description", task_description.clone()))
                                .await {
                                    Ok(mut r) => { 
                                        let res = r.take::<Option<Record>>(0);
                                        log::info!("updating description: {res:?}");
                                    },
                                    Err(e) => log::error!("Error updating description: {e:?}"),
                                };
                            });
                        }
                    });
                    ui.end_row();
                });
            });
        });
    });
}

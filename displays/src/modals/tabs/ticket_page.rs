use eframe::egui::{Button, Color32, ComboBox, FontId, Grid, Hyperlink, Id, Margin, RichText, ScrollArea, Spinner, TextEdit, Ui, Vec2, Widget};
use database::schema::{CarboniteResponse, ComputerData, CustomerData, LiveTaskPayload, Record, RecordIdExt, Status, TicketData, User};
use database::schema::prestashop::{Prestashop, OrderState};
use database::schema::prestashop::xml::{modify_xml, remove_xml_tag};
use database::schema::helper_traits::parse_email_user;
use database::{DATABASE, ReqwestClient};
use crate::{tabs::task_audit::row_viewer::BASE_URL, Interaction, PlatformSpawner, Spawner};
use crossbeam::channel::Sender;
use chrono::{DateTime, Utc};

use super::return_colors;

/// Helper function to update an order field via Prestashop API
async fn update_order_field(order_id: &str, field: &str, new_value: &str) {
    let api = Prestashop::default();
    match api.request_raw_resource_by_id("orders", order_id).await {
        Ok(xml) => {
            match modify_xml(&xml, field, new_value) {
                Ok(new_xml) => {
                    match remove_xml_tag(&new_xml, "tax_exempt") {
                        Ok(final_xml) => {
                            match api.modify_prestashop_order(&final_xml).await {
                                Ok(_) => log::info!("Successfully updated {} to {} for order {}", field, new_value, order_id),
                                Err(e) => log::error!("Error modifying prestashop order: {e:?}"),
                            }
                        }
                        Err(e) => log::error!("Error removing tax_exempt tag: {e:?}"),
                    }
                }
                Err(e) => log::error!("Error modifying XML: {e:?}")
            }
        },
        Err(e) => log::error!("Error getting XML order: {e:?}"),
    }
}

pub fn display_ticket_page(
    ui: &mut Ui, 
    task: &mut LiveTaskPayload, 
    service_ticket: Option<&mut TicketData>,
    customer: Option<&mut CustomerData>,
    computer: Option<&mut ComputerData>,
    avail_size: Vec2, 
    store_users: &Vec<User>, 
    current_user: User,
    seb_tx: Option<Sender<Vec<CarboniteResponse>>>,
    seb_checking: &mut bool,
    customer_modal_open: Option<&mut bool>,
) {
    // Check if this is a QC task
    let is_qc = task.status == Status::Qc;
    ui.vertical_centered_justified(|ui| {
        ui.colored_label(Color32::LIGHT_GREEN, format!("ID: {}", task.id.key_string()));

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
                ui.push_id(format!("Assignee {}", task.assignee.key_string()), |ui| {
                    task.interact_assignee(ui, store_users, &current_user);
                });
                
                ui.colored_label(Color32::LIGHT_RED, "Status");
                ui.push_id(format!("Status {}", task.status.as_str()), |ui| {
                    task.interact_status(&current_user, ui);
                });
                
                ui.end_row();

                let customer = if let Some(customer) = customer { customer }  else { &mut CustomerData::default() };

                ui.colored_label(Color32::LIGHT_RED, "Technician");
                // Technician ComboBox
                if let Some(service_number) = task.service_number.as_ref() {
                    let current_tech = ticket.tech.clone();
                    let order_id_for_combo = service_number.clone();
                    let is_checkin = current_tech == "checkinshelf" || current_tech.is_empty();
                    ComboBox::from_id_salt(Id::new(format!("tech_combo_{}", task.id.key_string())))
                        .selected_text(if current_tech.is_empty() { "Check-in Shelf" } else { current_tech.as_str() })
                        .width(100.)
                        .height(200.)
                        .show_ui(ui, |ui| {
                            // Add Check-in Shelf option
                            if ui.selectable_label(is_checkin, "Check-in Shelf").clicked() && !is_checkin {
                                let order_id = order_id_for_combo.clone();
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id, "id_employee_sales_rep", "1347").await;
                                });
                            }
                            ui.separator();
                            for user in store_users.iter().filter(|u| u.is_active()) {
                                let username = user.get_username();
                                let is_selected = username.to_lowercase() == current_tech.to_lowercase();
                                if ui.selectable_label(is_selected, username).clicked() && !is_selected {
                                    if let Some(emp_id) = user.get_employee_id() {
                                        let order_id = order_id_for_combo.clone();
                                        let emp_id_str = emp_id.to_string();
                                        log::info!("Updating tech to {} for order {}", emp_id_str, order_id);
                                        PlatformSpawner::spawn(async move {
                                            update_order_field(&order_id, "id_employee_sales_rep", &emp_id_str).await;
                                        });
                                    }
                                }
                            }
                        });
                } else {
                    ui.label(&ticket.tech);
                }
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
                // Salesman (Split Rep in prestashop terms) ComboBox
                if let Some(service_number) = task.service_number.as_ref() {
                    let current_salesman = ticket.salesman.clone();
                    let order_id_for_combo = service_number.clone();
                    let is_none = current_salesman.is_empty();
                    ComboBox::from_id_salt(Id::new(format!("salesman_combo_{}", task.id.key_string())))
                        .selected_text(if current_salesman.is_empty() { "None" } else { current_salesman.as_str() })
                        .width(100.)
                        .height(200.)
                        .show_ui(ui, |ui| {
                            // Add None option
                            if ui.selectable_label(is_none, "None").clicked() && !is_none {
                                let order_id = order_id_for_combo.clone();
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id, "id_employee_split_rep", "0").await;
                                });
                            }
                            ui.separator();
                            for user in store_users.iter().filter(|u| u.is_active()) {
                                let username = user.get_username();
                                let is_selected = username.to_lowercase() == current_salesman.to_lowercase();
                                if ui.selectable_label(is_selected, username).clicked() && !is_selected {
                                    if let Some(emp_id) = user.get_employee_id() {
                                        let order_id = order_id_for_combo.clone();
                                        let emp_id_str = emp_id.to_string();
                                        log::info!("Updating salesman (split_rep) to {} for order {}", emp_id_str, order_id);
                                        PlatformSpawner::spawn(async move {
                                            update_order_field(&order_id, "id_employee_split_rep", &emp_id_str).await;
                                        });
                                    }
                                }
                            }
                        });
                } else {
                    ui.label(&ticket.salesman);
                }
                ui.colored_label(Color32::LIGHT_RED, "Name");
                // Customer name as button to open change modal (only if we have a service number)
                if task.service_number.is_some() {
                    if let Some(modal_open) = customer_modal_open {
                        let customer_display = format!("[{}] {}", customer.cust_code, customer.name);
                        if ui.button(&customer_display).on_hover_text("Click to change customer").clicked() {
                            *modal_open = true;
                        }
                    } else {
                        ui.label(&customer.name);
                    }
                } else {
                    ui.label(&customer.name);
                }
                ui.end_row();

                ui.colored_label(Color32::LIGHT_RED, "Split Rep");
                // Split Rep (sales_rep field maps to this) ComboBox
                if let Some(service_number) = task.service_number.as_ref() {
                    let current_split = &ticket.sales_rep;
                    ComboBox::from_id_salt(Id::new(format!("splitrep_combo_{}", task.id.key_string())))
                        .selected_text(if current_split.is_empty() { "None" } else { current_split.as_str() })
                        .width(100.)
                        .height(200.)
                        .show_ui(ui, |ui| {
                            // Note: In ticket_page, sales_rep is the Split Rep display
                            // This is just for display - actual split rep changes are handled by salesman field
                            ui.label(RichText::new("(Read-only)").small().color(Color32::GRAY));
                        });
                } else {
                    ui.label(&ticket.sales_rep);
                }
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
                
                // Check SEB button row (for QC tasks or any task with customer email)
                if is_qc || !customer.email.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, "SEB Check");
                    ui.horizontal(|ui| {
                        let can_check = !customer.email.is_empty() && !*seb_checking;
                        if ui.add_enabled(can_check, Button::new("🔍 Check SEB")).clicked() {
                            if let Some(tx) = seb_tx.clone() {
                                *seb_checking = true;
                                let customer_email = customer.email.clone();
                                PlatformSpawner::spawn(async move {
                                    log::info!("Checking SEB for customer: {}", customer_email);
                                    let client = ReqwestClient::new();
                                    let response = CarboniteResponse::default()
                                        .from_customer_email(customer_email, client)
                                        .await;
                                    
                                    match response {
                                        Ok(seb_results) => {
                                            log::info!("SEB check returned {} results", seb_results.len());
                                            let _ = tx.try_send(seb_results);
                                        },
                                        Err(e) => log::error!("SEB check error: {:?}", e),
                                    }
                                });
                            }
                        }
                        if *seb_checking {
                            Spinner::new().size(16.0).ui(ui);
                        }
                    });
                    ui.label(""); // Empty cell
                    ui.label(""); // Empty cell
                    ui.end_row();
                }
            });
        });

        ui.add_space(15.);

        ui.group(|ui| {
            ScrollArea::vertical()
            .max_height(avail_size.y/1.3)
            .show(ui, |ui| 
            {
                if is_qc {
                    // QC mode: Show only QC Notes (full width)
                    ui.vertical_centered_justified(|ui| {
                        ui.label(
                            RichText::new("QC Notes:")
                                .font(FontId::proportional(15.0))
                                .color(Color32::GOLD)
                        );

                        ui.add_space(10.);

                        let res = TextEdit::multiline(&mut task.task_description)
                            .margin(Margin::symmetric(10, 3))
                            .desired_rows(20)
                            .desired_width(ui.available_width())
                            .ui(ui);

                        if res.lost_focus() {
                            let task_description = task.task_description.clone();
                            let task_id = task.id.clone();
                            PlatformSpawner::spawn(async move {
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
                    
                    // Show SEB info if available (for QC tasks)
                    if let Some(computer) = computer {
                        if let Some(seb_info) = &computer.seb_info {
                            ui.add_space(15.);
                            ui.separator();
                            ui.add_space(10.);
                            
                            ui.label(RichText::new("SEB Information:").font(FontId::proportional(15.0)).color(Color32::LIGHT_BLUE));
                            ui.add_space(5.);
                            
                            Grid::new("QC SEB Info").spacing(Vec2::new(4., 4.)).show(ui, |ui| {
                                ui.colored_label(Color32::LIGHT_RED, "Machine Name:");
                                ui.label(&seb_info.MachineName);
                                ui.end_row();
                                ui.colored_label(Color32::LIGHT_RED, "Install Stage:");
                                ui.label(&seb_info.InstallationStage);
                                ui.end_row();
                                ui.colored_label(Color32::LIGHT_RED, "Has Issues:");
                                ui.label(&seb_info.HasIssues);
                                ui.end_row();
                                ui.colored_label(Color32::LIGHT_RED, "Activation Code:");
                                ui.label(&seb_info.ActivationCode);
                                ui.end_row();
                            });
                        }
                    }
                } else {
                    // Normal mode: Show Checkin Notes and Recommendations side by side
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
                }
            });
        });
    });
}

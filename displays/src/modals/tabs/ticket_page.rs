use eframe::egui::{Color32, Grid, Label, Response, RichText, ScrollArea, Ui, Vec2};
use database::schema::{CarboniteResponse, ComputerData, CustomerData, LiveTaskPayload, RecordIdExt, Status, TicketData, User, assignable_users};
use database::schema::prestashop::OrderState;
use database::schema::prestashop::order_write;
// use database::schema::helper_traits::parse_email_user;
use database::xidax_order_url;
use reqwest::Client;
use super::bar_scroll;
use crate::{Interaction, PlatformSpawner, Spawner};
use crate::ui_tools::{
    controls::{
        self, IconBox, IconButton, IconLabel, IconLink, Metrics, SpinnerSize, TextArea, TextButton,
        TextField, Tokens,
    },
    glass_card,
    icons::{self, svg},
};
use crossbeam::channel::Sender;
use chrono::{DateTime, Utc};

/// Width of the shelf and staff dropdowns.
const PICKER_W: f32 = 150.0;
/// Width of the customer-id field.
const CUSTOMER_ID_W: f32 = 90.0;
/// Share of a label/value column pair given to the label.
const LABEL_SHARE: f32 = 0.4;

/// Runs `add` in a grid cell `width` wide.
fn grid_cell<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        add(ui)
    })
    .inner
}

/// `text` on one row cut with '…' at the available width and shown whole on hover.
fn truncated(ui: &mut Ui, text: &str) -> Response {
    ui.add(Label::new(text).truncate())
}

/// A `tone` label cell `width` wide, cut with '…' when longer.
fn label_cell(ui: &mut Ui, width: f32, tone: Color32, text: &str) -> Response {
    grid_cell(ui, width, |ui| ui.add(Label::new(RichText::new(text).color(tone)).truncate()))
}

/// `total` as dollars to the cent, or as given when it is not a number.
fn dollars(total: &str) -> String {
    match total.trim().parse::<f64>() {
        Ok(amount) => format!("${amount:.2}"),
        Err(_) => total.to_string(),
    }
}

/// Writes one order field through the serialized writer shared with the audit table.
async fn update_order_field(order_id: &str, field: &str, new_value: &str) {
    if let Err(e) = order_write::set_order_field(order_id, field, new_value).await {
        log::error!("Failed to set {field}={new_value} on order {order_id}: {e:?}");
    }
}

/// Dropdown options for picking a staff member: `first`, then every assignable user keyed by lowercase username.
fn staff_options<'a>(
    first: (&str, &'a str),
    users: &[&'a User],
    current: &str,
    current_label: &'a str,
) -> Vec<(String, &'a str)> {
    let mut options = vec![(first.0.to_string(), first.1)];
    options.extend(
        users
            .iter()
            .map(|u| (u.get_username().to_lowercase(), u.get_username())),
    );
    if !options.iter().any(|(key, _)| key == current) {
        options.push((current.to_string(), current_label));
    }
    options
}

/// The assignable user whose lowercase username is `key`.
fn user_by_key<'a>(users: &[&'a User], key: &str) -> Option<&'a User> {
    users
        .iter()
        .copied()
        .find(|u| u.get_username().to_lowercase() == key)
}

/// Writes the task's description to the database.
fn save_description(task: &LiveTaskPayload) {
    let task = task.clone();
    PlatformSpawner::spawn(async move {
        if let Err(e) = task.update_task_description().await {
            log::error!("Error updating description: {e:?}");
        }
    });
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
    service_history_open: Option<&mut bool>,
    order_current_state: Option<&mut String>,
) {
    // Check if this is a QC task
    let is_qc = task.status == Status::Qc;
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    ui.vertical_centered_justified(|ui| {
        controls::icon_label(ui, svg::HASH, &format!("ID: {}", task.id.key_string()));

        ui.add_space(m.gap.y);

        let ticket = if let Some(ticket) = service_ticket { ticket }  else { &mut TicketData::default() };
        glass_card::row_card(ui, |ui| {
            let spacing = m.gap.x / 4.0;
            let pair_w = (ui.available_width() - 3.0 * spacing) / 2.0;
            let label_w = (pair_w * LABEL_SHARE).floor();
            let value_w = pair_w - label_w;
            let label = |ui: &mut Ui, text: &str| label_cell(ui, label_w, t.text_dim, text);
            let value = |ui: &mut Ui, text: &str| grid_cell(ui, value_w, |ui| truncated(ui, text));
            Grid::new("Task Modal - Task Info Page")
                .spacing(Vec2::new(spacing, m.pad.y))
                .min_col_width(label_w)
                .max_col_width(value_w)
                .striped(true)
                .num_columns(4)
                .show(ui, |ui|
            {
                if current_user.is_admin() {
                    label(ui, "Service #");
                    grid_cell(ui, value_w, |ui| {
                        ui.push_id(format!("Service #{:?}", task.service_number), |ui| {
                            ui.set_width(PICKER_W);
                            task.interact_service_number(ui);
                        });
                    });
                } else {
                    label(ui, "");
                    value(ui, "");
                }

                if let Some(service_number) = task.service_number.as_ref() {
                    label(ui, "Prestashop Order");
                    let url = xidax_order_url(service_number);
                    grid_cell(ui, value_w, |ui| {
                        let _ = IconLink::new(service_number)
                            .icon(svg::ARROW_SQUARE_OUT)
                            .url(&url)
                            .new_tab(true)
                            .show(ui);
                    });
                } else {
                    label(ui, "");
                    value(ui, "");
                }

                ui.end_row();

                label(ui, "Assignee");
                grid_cell(ui, value_w, |ui| {
                    ui.push_id(format!("Assignee {}", task.assignee.key_string()), |ui| {
                        task.interact_assignee(ui, store_users, &current_user);
                    });
                });

                label(ui, "Status");
                grid_cell(ui, value_w, |ui| {
                    ui.push_id(format!("Status {}", task.status.as_str()), |ui| {
                        task.interact_status(&current_user, ui);
                    });
                });

                ui.end_row();

                label(ui, "Shelf");
                grid_cell(ui, value_w, |ui| {
                    if let Some(service_number) = task.service_number.as_ref() {
                        let current_state = order_current_state.as_deref().cloned().unwrap_or_default();
                        let states = [OrderState::CheckinShelf, OrderState::InRepair, OrderState::DoneShelf];
                        let mut options: Vec<(String, &str)> = states
                            .iter()
                            .map(|s| (s.to_id_str().to_string(), s.as_str()))
                            .collect();
                        if !options.iter().any(|(id, _)| *id == current_state) {
                            let shown = if current_state.is_empty() {
                                "Loading..."
                            } else {
                                OrderState::from_id_str(&current_state)
                            };
                            options.push((current_state.clone(), shown));
                        }
                        let mut picked = current_state.clone();
                        let salt = format!("shelf_combo_{}", task.id.key_string());
                        if controls::dropdown(ui, salt, &mut picked, &options, PICKER_W).changed()
                            && picked != current_state
                        {
                            let order_id_clone = service_number.clone();
                            let new_state = picked.clone();
                            log::info!("Shelf changed for order: {order_id_clone}, new_state: {new_state}");
                            PlatformSpawner::spawn(async move {
                                update_order_field(&order_id_clone, "current_state", &new_state).await;
                            });
                            if let Some(current) = order_current_state {
                                *current = picked;
                            }
                        }
                    } else {
                        truncated(ui, "—");
                    }
                });
                label(ui, "");
                value(ui, "");
                ui.end_row();

                let customer = if let Some(customer) = customer { customer }  else { &mut CustomerData::default() };
                let users = assignable_users(store_users, current_user.get_store());

                label(ui, "Technician");
                grid_cell(ui, value_w, |ui| {
                    if let Some(service_number) = task.service_number.as_ref() {
                        let current_tech = ticket.tech.clone();
                        let is_checkin = current_tech == "checkinshelf" || current_tech.is_empty();
                        let current = if is_checkin { "checkinshelf".to_string() } else { current_tech.to_lowercase() };
                        let options = staff_options(("checkinshelf", "Check-in Shelf"), &users, &current, &current_tech);
                        let mut picked = current.clone();
                        let salt = format!("tech_combo_{}", task.id.key_string());
                        if controls::dropdown(ui, salt, &mut picked, &options, PICKER_W).changed()
                            && picked != current
                        {
                            let order_id = service_number.clone();
                            if picked == "checkinshelf" {
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id, "id_employee_sales_rep", "1347").await;
                                });
                            } else if let Some(emp_id) = user_by_key(&users, &picked).and_then(|u| u.get_employee_id()) {
                                let emp_id_str = emp_id.to_string();
                                log::info!("Updating tech to {} for order {}", emp_id_str, order_id);
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id, "id_employee_sales_rep", &emp_id_str).await;
                                });
                            }
                        }
                    } else {
                        truncated(ui, &ticket.tech);
                    }
                });
                label(ui, "Customer ID");
                grid_cell(ui, value_w, |ui| {
                    TextField::new(&mut customer.cust_code)
                        .width(CUSTOMER_ID_W)
                        .icon(svg::IDENTIFICATION_CARD)
                        .show(ui);

                    if IconButton::new(svg::FLOPPY_DISK, IconBox::Md).tip("Save customer ID").show(ui).clicked() {
                        let id_customer = customer.cust_code.clone();
                        PlatformSpawner::spawn(async move {
                            let cust_data = CustomerData::find_customer_by_id(&id_customer).await;
                            log::info!("Cust Data {cust_data:?}");
                        });
                    }
                    if let Some(sho) = service_history_open
                        && IconButton::new(svg::CLOCK_COUNTER_CLOCKWISE, IconBox::Md)
                            .tip("View all services for this customer")
                            .show(ui)
                            .clicked()
                    {
                        *sho = true;
                    }
                });

                ui.end_row();

                label(ui, "Salesman");
                // Salesman (Split Rep in prestashop terms) dropdown
                grid_cell(ui, value_w, |ui| {
                    if let Some(service_number) = task.service_number.as_ref() {
                        let current_salesman = ticket.salesman.clone();
                        let current = current_salesman.to_lowercase();
                        let options = staff_options(("", "None"), &users, &current, &current_salesman);
                        let mut picked = current.clone();
                        let salt = format!("salesman_combo_{}", task.id.key_string());
                        if controls::dropdown(ui, salt, &mut picked, &options, PICKER_W).changed()
                            && picked != current
                        {
                            let order_id = service_number.clone();
                            if picked.is_empty() {
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id, "id_employee_split_rep", "0").await;
                                });
                            } else if let Some(emp_id) = user_by_key(&users, &picked).and_then(|u| u.get_employee_id()) {
                                let emp_id_str = emp_id.to_string();
                                log::info!("Updating salesman (split_rep) to {} for order {}", emp_id_str, order_id);
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id, "id_employee_split_rep", &emp_id_str).await;
                                });
                            }
                        }
                    } else {
                        truncated(ui, &ticket.salesman);
                    }
                });
                label(ui, "Name");
                // Customer name as button to open change modal (only if we have a service number)
                grid_cell(ui, value_w, |ui| {
                    match customer_modal_open.filter(|_| task.service_number.is_some()) {
                        Some(modal_open) => {
                            let customer_display = format!("[{}] {}", customer.cust_code, customer.name);
                            if TextButton::new(&customer_display)
                                .icon(svg::USER_SWITCH)
                                .max_width(value_w)
                                .show(ui)
                                .on_hover_text("Click to change customer")
                                .clicked()
                            {
                                *modal_open = true;
                            }
                        }
                        None => {
                            truncated(ui, &customer.name);
                        }
                    }
                });
                ui.end_row();

                label(ui, "Split Rep");
                grid_cell(ui, value_w, |ui| {
                    if task.service_number.is_some() {
                        let current_split = &ticket.sales_rep;
                        let shown = if current_split.is_empty() { "None" } else { current_split.as_str() };
                        IconLabel::new(svg::LOCK, shown).truncate(true).show(ui).on_hover_text("Read-only");
                    } else {
                        truncated(ui, &ticket.sales_rep);
                    }
                });
                label(ui, "Phone#");
                value(ui, &customer.phone_number);
                ui.end_row();

                label(ui, "Terms");
                value(ui, &ticket.terms);
                label(ui, "Phone2");
                value(ui, &customer.phone_number_2);
                ui.end_row();

                label(ui, "Total on Order");
                value(ui, &dollars(&ticket.ticket_total));
                label(ui, "Email");
                value(ui, &customer.email);
                ui.end_row();

                label(ui, "Tur Sent:");
                let date: DateTime<Utc> = ticket.created_at.into();
                value(ui, &date.date_naive().to_string());

                label(ui, "Due Date");
                grid_cell(ui, value_w, |ui| {
                    ui.push_id(format!("Due Date {}", task.due_date), |ui| {
                        task.interact_due_date(ui);
                    });
                });
                ui.end_row();

                // Check SEB button row (for QC tasks or any task with customer email)
                if is_qc || !customer.email.is_empty() {
                    label(ui, "SEB Check");
                    grid_cell(ui, value_w, |ui| {
                        let can_check = !customer.email.is_empty() && !*seb_checking;
                        let clicked = TextButton::new("Check SEB")
                            .icon(svg::CLOUD_CHECK)
                            .enabled(can_check)
                            .show(ui)
                            .clicked();
                        if clicked && let Some(tx) = seb_tx.clone() {
                            *seb_checking = true;
                            let customer_email = customer.email.clone();
                            PlatformSpawner::spawn(async move {
                                log::info!("Checking SEB for customer: {}", customer_email);
                                let client = Client::new();
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
                        if *seb_checking {
                            controls::spinner(ui, SpinnerSize::Sm);
                        }
                    });
                    label(ui, "");
                    value(ui, "");
                    ui.end_row();
                }
            });
        });

        ui.add_space(2.0 * m.gap.y);

        glass_card::row_card(ui, |ui| {
            let bar = ui.spacing().scroll.allocated_width();
            bar_scroll(ui, ScrollArea::vertical().max_height(avail_size.y / 1.3), bar, |ui|
            {
                if is_qc {
                    // QC mode: Show only QC Notes (full width)
                    ui.vertical_centered_justified(|ui| {
                        glass_card::card_header(ui, icons::p::CLIPBOARD_TEXT, "QC Notes", None);

                        ui.add_space(m.gap.y);

                        let res = TextArea::new(&mut task.task_description).rows(20).show(ui);

                        if res.lost_focus() {
                            save_description(task);
                        }
                    });

                    // Show SEB info if available (for QC tasks)
                    if let Some(computer) = computer {
                        if let Some(seb_info) = &computer.seb_info {
                            ui.add_space(2.0 * m.gap.y);
                            glass_card::card_header(ui, icons::p::CLOUD_CHECK, "SEB Information", None);
                            ui.add_space(m.pad.y);

                            Grid::new("QC SEB Info").spacing(Vec2::new(m.pad.y, m.pad.y)).show(ui, |ui| {
                                ui.colored_label(t.text_dim, "Machine Name:");
                                ui.label(&seb_info.MachineName);
                                ui.end_row();
                                ui.colored_label(t.text_dim, "Install Stage:");
                                ui.label(&seb_info.InstallationStage);
                                ui.end_row();
                                ui.colored_label(t.text_dim, "Has Issues:");
                                ui.label(&seb_info.HasIssues);
                                ui.end_row();
                                ui.colored_label(t.text_dim, "Activation Code:");
                                ui.label(&seb_info.ActivationCode);
                                ui.end_row();
                            });
                        }
                    }
                } else {
                    // Normal mode: Show Checkin Notes and Recommendations side by side
                    let spacing = m.gap.x / 4.0;
                    let col_w = (ui.available_width() - spacing) / 2.0;
                    Grid::new("Checkin Notes and Recommendations")
                    .spacing(Vec2::new(spacing, m.pad.y))
                    .max_col_width(col_w)
                    .min_col_width(col_w)
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.vertical_centered_justified(|ui| {
                            glass_card::card_header(ui, icons::p::NOTE, "Checkin Notes", None);

                            ui.add_space(m.gap.y);

                            TextArea::new(&mut ticket.checkin_notes).rows(15).show(ui);
                        });

                        ui.vertical_centered_justified(|ui| {
                            glass_card::card_header(ui, icons::p::LIGHTBULB, "Recommendations", None);

                            ui.add_space(m.gap.y);

                            let res = TextArea::new(&mut task.task_description).rows(15).show(ui);

                            if res.lost_focus() {
                                save_description(task);
                            }
                        });
                        ui.end_row();
                    });
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::dollars;

    #[test]
    fn order_totals_show_dollars_to_the_cent() {
        assert_eq!(dollars("149.99"), "$149.99");
        assert_eq!(dollars("1250"), "$1250.00");
        assert_eq!(dollars(" 7.5 "), "$7.50");
        assert_eq!(dollars(""), "");
        assert_eq!(dollars("n/a"), "n/a");
    }
}

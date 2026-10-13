use crate::Spawner;
use crate::ui_tools::controls::IconLabel;
use crate::ui_tools::controls::{
    self, BarKind, ButtonKind, IconBox, MenuButton, SpinnerSize, TextButton,
};
use crate::ui_tools::icons::{self, svg};
use crate::ui_tools::metrics::Metrics;
use crate::ui_tools::tokens::Tokens;
use crate::{PlatformSpawner, TaskUiActions};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use crossbeam::channel::Sender;
use database::get_database_users;
use database::schema::prestashop::{OrderState, OrderType};
use database::schema::{Store, User};
use eframe::egui::{CentralPanel, Id, Label, Layout, RichText, ScrollArea, Ui, Widget};
use eframe::egui::{Grid, scroll_area};
use egui_data_table::Renderer;
use log::info;

use super::TaskAudit;
use super::TaskAuditViewer;
use super::row_viewer::TaskRowViewer;

impl TaskAuditViewer {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        current_user: Option<User>,
        ui_actions_tx: Sender<TaskUiActions>,
    ) {
        // First run: load database users for the rep pickers
        if self.services_viewer.first_run {
            let users = get_database_users();
            if !users.is_empty() {
                self.services_viewer.users = users;
                self.services_viewer.my_store = current_user
                    .as_ref()
                    .map(|u| u.get_store())
                    .unwrap_or_default();
                self.services_viewer.first_run = false;
                info!(
                    "Loaded {} users for task audit comboboxes",
                    self.services_viewer.users.len()
                );
            }
        }

        // Handle create task channel
        if let Ok(order_payload) = self.services_viewer.create_task_channel.1.try_recv() {
            info!(
                "Received create task request for order: {}",
                order_payload.order.id
            );
            // Send the order to the task creation modal
            let _ =
                ui_actions_tx.try_send(TaskUiActions::OpenCreateTaskModalFromOrder(order_payload));
        }

        // Handle open existing task channel
        if let Ok(task) = self.services_viewer.open_task_channel.1.try_recv() {
            info!("Received open task request for task: {}", task.task_name);
            let _ = ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task));
        }

        if let Some(order) = self.services_viewer.selected.clone() {
            let header = &format!("{} - {}", order.customer.name, order.order.id);

            let mut side_panel_open = self.services_viewer.selected.is_some();
            eframe::egui::Panel::right(Id::new("Task Audit Side Panel"))
                .default_size(280.)
                .max_size(900.)
                .resizable(true)
                .show_separator_line(true)
                .show_collapsible(ui, &mut side_panel_open, |ui| {
                    let t = Tokens::get(ui.ctx());
                    let m = Metrics::get(ui.ctx());
                    ui.vertical_centered_justified(|ui| {
                        controls::panel_header(ui, svg::RECEIPT, &header.to_uppercase(), |_| {});

                        controls::bar(ui, BarKind::Tool, |ui| {
                            if controls::icon_text_button(ui, svg::X, "Close").clicked() {
                                self.services_viewer.selected = None;
                            }

                            ui.with_layout(
                                Layout::right_to_left(eframe::egui::Align::Center),
                                |ui| {
                                    let create = TextButton::new("Create Task")
                                        .icon(icons::CREATE_TASK)
                                        .kind(ButtonKind::Primary)
                                        .show(ui);
                                    if create.clicked() {
                                        let tx = self.services_viewer.tur_channel.0.clone();
                                        let order_num = order.order.id.clone();
                                        PlatformSpawner::spawn(async move {
                                            let presta_order =
                                                TaskRowViewer::get_prestashop_order(order_num)
                                                    .await
                                                    .unwrap_or_default();
                                            let _ = tx.try_send(presta_order);
                                        });
                                    }
                                },
                            );
                        });

                        ui.add_space(m.gap.y);

                        // Order details capped at half the remaining height.
                        ScrollArea::vertical()
                            .auto_shrink(true)
                            .max_height(ui.available_height() * 0.5)
                            .show(ui, |ui| {
                                ui.group(|ui| {
                                    Grid::new(order.order.id.clone())
                                        .num_columns(2)
                                        .min_col_width(ui.available_width() / 2.1)
                                        .max_col_width(425.)
                                        .striped(true)
                                        .show(ui, |ui| {
                                            IconLabel::new(svg::TAG, "Status")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);

                                            let order_type = OrderState::from_id_str(
                                                order.order.current_state.as_str(),
                                            );

                                            ui.label(order_type);
                                            ui.end_row();

                                            IconLabel::new(svg::PHONE, "Phone#")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);
                                            let phone = order.customer.phone_number.clone();
                                            ui.label(phone);
                                            ui.end_row();

                                            IconLabel::new(svg::ENVELOPE_SIMPLE, "Email")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);
                                            let phone = order.customer.email.clone();
                                            ui.label(phone);
                                            ui.end_row();

                                            IconLabel::new(svg::USER, "Sales Rep")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);
                                            let sales_rep = order.sales_rep.unwrap_or_default();
                                            ui.label(format!(
                                                "{} {}",
                                                sales_rep.firstname, sales_rep.lastname
                                            ));
                                            ui.end_row();

                                            IconLabel::new(svg::USERS, "Split Rep")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);
                                            let split_rep = order.split_rep.unwrap_or_default();
                                            ui.label(format!(
                                                "{} {}",
                                                split_rep.firstname, split_rep.lastname
                                            ));
                                            ui.end_row();

                                            // Parse the input into a NaiveDateTime
                                            let naive_datetime = NaiveDateTime::parse_from_str(
                                                &order.order.date_add,
                                                "%Y-%m-%d %H:%M:%S",
                                            )
                                            .unwrap_or_default();
                                            // Convert to a DateTime with Utc timezone
                                            let datetime: DateTime<Utc> =
                                                DateTime::from_naive_utc_and_offset(
                                                    naive_datetime,
                                                    Utc,
                                                );
                                            // Format the DateTime into yyyy/mm/dd
                                            let formatted_date =
                                                datetime.format(" %m/%d/%Y").to_string();

                                            IconLabel::new(svg::CALENDAR, "Check-in Date")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);
                                            ui.label(formatted_date);
                                            ui.end_row();

                                            let missing_days = self
                                                .services_viewer
                                                .missed_calls
                                                .iter()
                                                .find(|o| o.id == order.order.id)
                                                .map(|call| call.missing_days.as_slice())
                                                .unwrap_or_default();

                                            IconLabel::new(svg::PHONE_X, "Missed Calls")
                                                .tone(Tokens::get(ui.ctx()).text_dim)
                                                .show(ui);
                                            if missing_days.is_empty() {
                                                ui.label("None");
                                            } else {
                                                ui.colored_label(
                                                    t.error,
                                                    missing_days.len().to_string(),
                                                );
                                            }
                                            ui.end_row();

                                            for (month, days) in missed_days_by_month(missing_days)
                                            {
                                                controls::IconLabel::new(svg::CARET_RIGHT, &month)
                                                    .tone(t.text_faint)
                                                    .show(ui);
                                                Label::new(RichText::new(days).color(t.error))
                                                    .wrap()
                                                    .ui(ui);
                                                ui.end_row();
                                            }
                                        });
                                });

                                ui.add_space(m.gap.y);
                            });
                    });

                    ui.with_layout(Layout::bottom_up(eframe::egui::Align::Min), |ui| {
                        self.services_viewer.chat_view.ui(ui);
                    });
                });
        }

        eframe::egui::Panel::top("Task Audit Top Panel").show(ui, |ui| {
            controls::bar(ui, BarKind::Tool, |ui| {
                controls::search_field(
                    ui,
                    &mut self.services_viewer.filter,
                    "Search SO# / Customer / Device / Model / Notes / Date",
                    320.0,
                );

                let stores: Vec<(u64, &str)> = Store::VALUES
                    .iter()
                    .map(|store| (store.into_store_id() as u64, store.as_str()))
                    .collect();
                controls::dropdown(
                    ui,
                    "TaskAudit Store Selection",
                    &mut self.services_viewer.store_selection,
                    &stores,
                    130.0,
                );

                let selected_text = match &self.audit_selection {
                    TaskAudit::MyInRepair => "My In Repair".to_string(),
                    TaskAudit::MyServices => "My Services".to_string(),
                    TaskAudit::NeedsCallToday => "Calls Due Today".to_string(),
                    TaskAudit::Status(OrderState::CheckinShelf) => "Check-in Shelf".to_string(),
                    TaskAudit::Status(OrderState::InRepair) => "In Repair".to_string(),
                    TaskAudit::Status(OrderState::DoneShelf) => "Done Shelf".to_string(),
                    TaskAudit::Status(state) => state.as_str().to_string(),
                    TaskAudit::AllExcept { order_type, .. } => match order_type {
                        OrderType::SalesOrder => "All Sales".to_string(),
                        _ => "All Service".to_string(),
                    },
                };

                MenuButton::new(&selected_text)
                    .icon(svg::LIST_BULLETS)
                    .tip("Which orders to list")
                    .show(ui, |ui| {
                        let fixed = [
                            (TaskAudit::MyInRepair, svg::WRENCH, "My In Repair"),
                            (TaskAudit::MyServices, svg::USER, "My Services"),
                            (
                                TaskAudit::NeedsCallToday,
                                svg::PHONE_OUTGOING,
                                "Calls Due Today",
                            ),
                            (
                                TaskAudit::Status(OrderState::CheckinShelf),
                                svg::TRAY_ARROW_DOWN,
                                "Check-in Shelf",
                            ),
                            (
                                TaskAudit::Status(OrderState::InRepair),
                                svg::SCREWDRIVER,
                                "In Repair",
                            ),
                            (
                                TaskAudit::Status(OrderState::DoneShelf),
                                svg::CHECK_SQUARE,
                                "Done Shelf",
                            ),
                        ];
                        for (choice, icon, label) in fixed {
                            let on = self.audit_selection == choice;
                            if controls::menu_check_item(ui, icon, label, on).clicked() {
                                self.audit_selection = choice;
                            }
                        }

                        let is_service = matches!(
                            &self.audit_selection,
                            TaskAudit::AllExcept {
                                order_type: OrderType::ServiceOrder,
                                ..
                            }
                        );
                        if controls::menu_check_item(ui, svg::STACK, "All Service", is_service)
                            .clicked()
                            && !is_service
                        {
                            self.audit_selection = TaskAudit::AllExcept {
                                order_type: OrderType::ServiceOrder,
                                excluded: Vec::new(),
                            };
                        }

                        let is_sales = matches!(
                            &self.audit_selection,
                            TaskAudit::AllExcept {
                                order_type: OrderType::SalesOrder,
                                ..
                            }
                        );
                        if controls::menu_check_item(ui, svg::SHOPPING_BAG, "All Sales", is_sales)
                            .clicked()
                            && !is_sales
                        {
                            self.audit_selection = TaskAudit::AllExcept {
                                order_type: OrderType::SalesOrder,
                                excluded: Vec::new(),
                            };
                        }
                    });

                if let TaskAudit::AllExcept {
                    order_type,
                    excluded,
                } = &mut self.audit_selection
                {
                    let applicable = order_type.applicable_states();
                    let exclude_text = if excluded.is_empty() {
                        "Exclude".to_string()
                    } else {
                        format!("Exclude ({})", excluded.len())
                    };
                    MenuButton::new(&exclude_text)
                        .icon(svg::FUNNEL_SIMPLE)
                        .keep_open(true)
                        .tip("Order states to leave out")
                        .show(ui, |ui| {
                            for state in applicable {
                                let is_excluded = excluded.contains(&state);
                                if controls::menu_check_item(ui, None, state.as_str(), is_excluded)
                                    .clicked()
                                {
                                    if is_excluded {
                                        excluded.retain(|s| s != &state);
                                    } else {
                                        excluded.push(state);
                                    }
                                }
                            }
                        });
                }

                crate::tabs::bar_rule(ui);

                if let Some(time) = self.time.clone() {
                    if time.elapsed() > web_time::Duration::from_secs(5) {
                        self.loading = false;
                    }
                }

                if controls::icon_text_button(ui, svg::DOWNLOAD_SIMPLE, "Load").clicked() {
                    self.loading = true;
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let key = self.current_key();
                    self.loading_key = key.clone();

                    let start_idx = self.index.entry(key.clone()).or_insert(0).clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&key) {
                        k.iter()
                            .map(|k| k.order.id.clone())
                            .collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(
                        selected.clone(),
                        current_user.clone(),
                        order_tx,
                        svcs,
                        start_idx,
                        self.missed_calls_tx.clone(),
                        self.services_viewer.store_selection.to_string(),
                    );
                }
                // Drops the rows on screen and re-pulls the selection from the
                // first page, so orders that moved out of it disappear instead
                // of lingering while the new ones append beneath them.
                if controls::icon_button(
                    ui,
                    svg::ARROW_CLOCKWISE,
                    IconBox::Md,
                    false,
                    "Reload this store's list from Prestashop",
                )
                .clicked()
                {
                    self.loading = true;
                    self.services_viewer.write_errors.clear();
                    self.services_viewer.missed_calls.clear();
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let key = self.current_key();
                    self.loading_key = key.clone();

                    self.service_map.remove(&key);
                    self.index.insert(key.clone(), 0);

                    self.time = Some(web_time::Instant::now());
                    Self::get_services(
                        selected.clone(),
                        current_user.clone(),
                        order_tx,
                        Vec::new(),
                        0,
                        self.missed_calls_tx.clone(),
                        self.services_viewer.store_selection.to_string(),
                    );
                }
                if controls::icon_text_button(ui, svg::PLUS, "Load +10").clicked() {
                    let order_tx = self.order_channel.0.clone();
                    let selected = self.audit_selection.clone();
                    let key = self.current_key();
                    self.loading_key = key.clone();

                    let start_idx = self
                        .index
                        .entry(key.clone())
                        .and_modify(|i| *i += 10)
                        .or_insert(0)
                        .clone();

                    let svcs = if let Some(k) = self.service_map.get_mut(&key) {
                        k.iter()
                            .map(|k| k.order.id.clone())
                            .collect::<Vec<String>>()
                    } else {
                        Vec::new()
                    };
                    self.time = Some(web_time::Instant::now());
                    Self::get_services(
                        selected.clone(),
                        current_user.clone(),
                        order_tx,
                        svcs,
                        start_idx,
                        self.missed_calls_tx.clone(),
                        self.services_viewer.store_selection.to_string(),
                    );
                }

                if self.loading {
                    ui.ctx().request_repaint();
                    controls::spinner(ui, SpinnerSize::Sm);
                }
            });
        });

        CentralPanel::default().show(ui, |ui| {
            let key = self.current_key();
            match self.service_map.get_mut(&key) {
                Some(table) => {
                    // style.single_click_edit_mode = true;
                    Renderer::new(table, &mut self.services_viewer)
                        .with_style_modify(|s| {
                            s.scroll_bar_visibility =
                                scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                }
                None if !self.loading => {
                    let store = Store::from_presta_store_id(
                        &self.services_viewer.store_selection.to_string(),
                    );
                    controls::empty_state(
                        ui,
                        svg::TRAY,
                        &format!("Nothing loaded for {}", store.as_str()),
                        "Press Load.",
                        &[],
                    );
                }
                None => {}
            }
        });
    }
}

/// Groups "YYYY-MM-DD" dates into (month name, space-separated day numbers) runs.
fn missed_days_by_month(days: &[String]) -> Vec<(String, String)> {
    let mut groups: Vec<((i32, u32), String, Vec<String>)> = Vec::new();
    for date in days
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        let key = (date.year(), date.month());
        let day = date.format("%d").to_string();
        match groups.last_mut() {
            Some((last, _, days)) if *last == key => days.push(day),
            _ => groups.push((key, date.format("%b").to_string(), vec![day])),
        }
    }
    groups
        .into_iter()
        .map(|(_, month, days)| (month, days.join(" ")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::missed_days_by_month;

    #[test]
    fn groups_days_by_month() {
        let days: Vec<String> = ["2026-09-29", "2026-09-30", "2026-10-01", "2026-10-02"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            missed_days_by_month(&days),
            vec![
                ("Sep".to_string(), "29 30".to_string()),
                ("Oct".to_string(), "01 02".to_string()),
            ]
        );
    }

    #[test]
    fn same_month_different_year_splits() {
        let days: Vec<String> = ["2025-10-31", "2026-10-01"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(missed_days_by_month(&days).len(), 2);
    }
}

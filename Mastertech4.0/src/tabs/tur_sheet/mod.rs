use crate::{
    app_state::MastertechContext,
    tabs::tur_sheet::scaffold::HardwareTest::{
        self, HddFail, HddNotTested, HddPass, RamFail, RamNotTested, RamPass, SsdFail,
        SsdNotTested, SsdPass,
    },
};
use database::schema::{
    CarboniteResponse, CustomerData, LiveTaskPayload, TicketData, assignee_names,
};
use displays::plugins::push_widget_anchor;
use displays::ui_tools::{
    autocomplete::AutoCompleteTextEdit,
    controls::{self, BarKind, ButtonKind, Metrics, TextArea, TextButton, TextField, Tokens},
    fonts, glass_card,
    icons::svg::{self, Icon},
    toasts::{Toast, ToastKind, ToastOptions},
};
use eframe::egui::{
    Grid, Key, KeyboardShortcut, Modifiers, Rect, ScrollArea, Stroke, StrokeKind, Ui, Widget, vec2,
};
use egui::Frame;
use get_ticket::SendRequest;
use log::{debug, info};
use std::f32;
use std::path::PathBuf;
use tokio::spawn;

pub mod assist_prompt;
pub mod get_ticket;
pub mod presta_api;
pub mod scaffold;
pub mod submit_tur_mtech;

/// Column width of the ticket grid.
const TICKET_COL_W: f32 = 160.0;
/// Column width of the hardware, device, SEB and product grids.
const SPEC_COL_W: f32 = 200.0;
/// Column width of the notes grid.
const NOTES_COL_W: f32 = 305.0;
/// Rows shown by the notes boxes.
const NOTES_ROWS: usize = 13;
/// Text size in the assignee boxes.
const ASSIGNEE_PT: f32 = 12.0;
/// Longest service number the field accepts.
const SERVICE_NUMBER_CHARS: usize = 11;
/// Placeholder phone number that does not count as filled in.
const PLACEHOLDER_PHONE: &str = "801-334-6262";

/// A one-line field with a leading icon at the grid's column width.
fn spec_field(ui: &mut Ui, text: &mut String, icon: Icon, hint: &str) -> eframe::egui::Response {
    TextField::new(text)
        .hint(hint)
        .icon(icon)
        .width(SPEC_COL_W)
        .show(ui)
}

/// Border painted around an assignee box, red while it is empty.
fn assignee_frame(ui: &Ui, rect: Rect, missing: bool) {
    let t = Tokens::get(ui.ctx());
    let color = if missing { t.error } else { t.field_border };
    ui.painter().rect_stroke(
        rect,
        t.radius_sm,
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
}

impl MastertechContext {
    pub fn tur_sheet(&mut self, ui: &mut Ui) {
        // Render confirmation toast if showing
        self.render_confirmation_toast(ui.ctx());

        // Process any pending duplicate check results
        self.process_duplicate_check_results();

        // Handle the merge modal if it's open
        self.handle_merge_modal(ui.ctx());

        self.render_assist_prompt(ui.ctx());

        let m = Metrics::get(ui.ctx());
        ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            let enabled = !self.ticket_data.service_number.is_empty()
                || !self.customer_data.phone_number.is_empty();

            controls::bar(ui, BarKind::Tool, |ui| {
                if TextButton::new("Reset Data")
                    .icon(svg::ARROW_COUNTER_CLOCKWISE)
                    .kind(ButtonKind::Danger)
                    .show(ui)
                    .clicked()
                {
                    self.ticket_data = TicketData::default();
                    self.task_data = LiveTaskPayload::default();
                    self.customer_data = CustomerData::default();
                    self.seb_info.clear();
                    self.order_rows.clear();
                    self.service_details.clear();
                    self.task_notes.clear();
                }

                let pull_btn = TextButton::new("Pull Order")
                    .icon(svg::CLOUD_ARROW_DOWN)
                    .kind(ButtonKind::Primary)
                    .enabled(enabled)
                    .show(ui)
                    .on_hover_text("Loads the order into the sheet from whichever backend has it.");
                push_widget_anchor("tur.pull_order", pull_btn.rect);
                // Old key kept so existing MCP automation still hits the button.
                push_widget_anchor("tur.get_prestashop_order", pull_btn.rect);
                if pull_btn.clicked() {
                    let service_num = self.ticket_data.service_number.clone();
                    let phone = self.customer_data.phone_number.clone();
                    self.pull_order();
                    self.ticket_data = TicketData::default();
                    self.task_data = LiveTaskPayload::default();
                    self.customer_data = CustomerData::default();
                    self.task_notes = Vec::new();
                    self.ticket_data.service_number = service_num;
                    // A phone-only pull clears the field it searched on, so
                    // the search term vanishes before the result lands.
                    self.customer_data.phone_number = phone;
                }
            });

            ui.add_space(m.gap.y);

            // Card padding and stroke on both sides.
            let chrome = 2.0 * (f32::from(m.window_margin) + 1.0);
            let left_w = 4.0 * TICKET_COL_W + 3.0 * m.gap.x + chrome;
            let right_w = 2.0 * SPEC_COL_W + m.gap.x + chrome;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    glass_card::titled_group(ui, svg::TICKET, "Ticket", None, |ui| self.ticket_info_grid(ui));
                    glass_card::titled_group(ui, svg::NOTE_PENCIL, "Notes", None, |ui| {
                        self.recommendations_grid(ui)
                    });
                });
                ui.vertical(|ui| {
                    ui.set_width(right_w);
                    glass_card::titled_group(ui, svg::CPU, "Hardware", None, |ui| self.computer_info_grid(ui));
                    glass_card::titled_group(ui, svg::LAPTOP, "Device Info", None, |ui| {
                        self.device_info_grid(ui)
                    });
                    glass_card::titled_group(ui, svg::CLOUD_CHECK, "SEB Info", None, |ui| {
                        self.seb_info_grid(ui)
                    });
                    glass_card::titled_group(ui, svg::PACKAGE, "Products", None, |ui| {
                        self.product_info_grid(ui)
                    });
                });
            });
        });
    }

    fn ticket_info_grid(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        let t = Tokens::get(ui.ctx());
        Grid::new("ticket_info_grid")
            .spacing(m.gap)
            .min_col_width(TICKET_COL_W)
            .max_col_width(TICKET_COL_W)
            .num_columns(4)
            .show(ui, |ui| {
                let service_number_check = self.ticket_data.service_number.is_empty();
                let name_check = self.customer_data.name.is_empty();
                let phone_number_check = self.customer_data.phone_number.is_empty()
                    || self.customer_data.phone_number == PLACEHOLDER_PHONE;
                let salesman_check = self.ticket_data.salesman.is_empty();
                let tech_check = self.ticket_data.tech.is_empty();

                let service_num = TextField::new(&mut self.ticket_data.service_number)
                    .id_salt("tur_sheet_service_number")
                    .hint("Service #")
                    .icon(svg::HASH)
                    .width(TICKET_COL_W)
                    .char_limit(SERVICE_NUMBER_CHARS)
                    .invalid(service_number_check)
                    .show(ui);
                push_widget_anchor("tur.service_number", service_num.rect);

                service_num
                    .clone()
                    .on_hover_text("(hint) Press Enter to pull ticket after typing SO#");

                let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                // Any non-blank reference, not 7+ characters: a Shopify order number
                // is four digits, and the length gate meant Enter never fired for one.
                if !self.ticket_data.service_number.trim().is_empty()
                    && accepted_by_keyboard
                    && service_num.lost_focus()
                {
                    let service_num = self.ticket_data.service_number.clone();
                    self.pull_order();
                    self.ticket_data = TicketData::default();
                    self.task_data = LiveTaskPayload::default();
                    self.customer_data = CustomerData::default();
                    self.task_notes = Vec::new();
                    self.ticket_data.service_number = service_num;
                }

                let tur_name = TextField::new(&mut self.customer_data.name)
                    .hint("Customer Name")
                    .icon(svg::USER)
                    .width(TICKET_COL_W)
                    .invalid(name_check)
                    .show(ui);
                push_widget_anchor("tur.customer_name", tur_name.rect);

                let tur_phone = TextField::new(&mut self.customer_data.phone_number)
                    .hint("Phone Number 1")
                    .icon(svg::PHONE)
                    .width(TICKET_COL_W)
                    .invalid(phone_number_check)
                    .show(ui);
                push_widget_anchor("tur.phone_number", tur_phone.rect);

                let tur_email = TextField::new(&mut self.customer_data.email)
                    .hint("Customer email")
                    .icon(svg::ENVELOPE_SIMPLE)
                    .width(TICKET_COL_W)
                    .show(ui);
                push_widget_anchor("tur.customer_email", tur_email.rect);

                ui.end_row();

                ui.label("");

                let roster = displays::get_database_users();
                let my_store = self
                    .shared_ctx
                    .current_user
                    .as_ref()
                    .map(|u| u.get_store())
                    .unwrap_or_default();
                let (inputs, local) = assignee_names(&roster, my_store);
                let local_tech = local.clone();
                let assignee_size = vec2(TICKET_COL_W, m.control_h);
                let field = t.field;

                let tur_salesman =
                    AutoCompleteTextEdit::new(&mut self.ticket_data.salesman, inputs.clone())
                        .highlight_matches(true)
                        .max_suggestions(3)
                        .group_by(move |name| usize::from(!local.contains(name)))
                        .set_text_edit_properties(move |text_edit| {
                            text_edit
                                .hint_text("Assignee")
                                .background_color(field)
                                .min_size(assignee_size)
                                .desired_rows(1)
                                .font(fonts::mono(ASSIGNEE_PT))
                                .frame(Frame::NONE)
                                .return_key(Some(KeyboardShortcut::new(Modifiers::CTRL, Key::Enter)))
                        })
                        .ui(ui);
                assignee_frame(ui, tur_salesman.rect, salesman_check);
                push_widget_anchor("tur.salesman", tur_salesman.rect);

                let tur_tech = AutoCompleteTextEdit::new(&mut self.ticket_data.tech, inputs)
                    .highlight_matches(true)
                    .max_suggestions(3)
                    .group_by(move |name| usize::from(!local_tech.contains(name)))
                    .set_text_edit_properties(move |text_edit| {
                        text_edit
                            .hint_text("Tech")
                            .background_color(field)
                            .min_size(assignee_size)
                            .font(fonts::mono(ASSIGNEE_PT))
                            .frame(egui::Frame::NONE)
                            .desired_rows(1)
                            .return_key(Some(KeyboardShortcut::new(Modifiers::CTRL, Key::Enter)))
                    })
                    .ui(ui);
                assignee_frame(ui, tur_tech.rect, tech_check);
                push_widget_anchor("tur.tech", tur_tech.rect);

                ui.label("");

                ui.end_row();
                ui.end_row();

                let check_seb = TextButton::new("Check SEB")
                    .icon(svg::CLOUD_CHECK)
                    .min_width(TICKET_COL_W)
                    .enabled(!self.ticket_data.service_number.is_empty())
                    .show(ui);
                if check_seb.clicked() {
                    let email = self.customer_data.email.clone();
                    let client = self.client.clone();
                    let tx = self.seb_channel.0.clone();
                    if !email.is_empty() {
                        tokio::spawn(async move {
                            let response_json: Vec<CarboniteResponse> = CarboniteResponse::default()
                                .from_customer_email(email.clone(), client)
                                .await?;
                            log::info!("SEB Response: {:?}", response_json);
                            tx.try_send(response_json)?;
                            Ok::<(), anyhow::Error>(())
                        });
                    }
                }

                let t = Tokens::get(ui.ctx());
                if TextButton::new(&self.keys.webroot_key)
                    .icon(svg::SHIELD_CHECK)
                    .min_width(TICKET_COL_W)
                    .ink(t.warn)
                    .mono(true)
                    .show(ui)
                    .on_hover_text("Click To Copy Webroot Key to Clipboard")
                    .clicked()
                {
                    let webroot = self.keys.webroot_key.clone();
                    ui.ctx().copy_text(webroot);
                }

                if TextButton::new(&self.keys.superanti_key)
                    .icon(svg::BUG_BEETLE)
                    .min_width(TICKET_COL_W)
                    .ink(t.error)
                    .mono(true)
                    .show(ui)
                    .on_hover_text("Click To Copy SAS Key to Clipboard")
                    .clicked()
                {
                    let sas = self.keys.superanti_key.clone();
                    ui.ctx().copy_text(sas);
                }

                let get_keys = TextButton::new("Get Keys")
                    .icon(svg::KEY)
                    .min_width(TICKET_COL_W)
                    .enabled(!self.ticket_data.service_number.is_empty())
                    .show(ui);
                if get_keys.clicked() {
                    self.spinner = true;
                    let client = self.client.clone();
                    let cps_tx = self.cps_keys_tx.clone();
                    let service_num = self.ticket_data.service_number.clone();

                    spawn(async move {
                        match cps_tx.try_send(
                            SendRequest::get_cps(service_num, client)
                                .await
                                .unwrap_or(vec![]),
                        ) {
                            Ok(_) => info!("GetKeysClick -> sent keys successfully"),
                            Err(err) => debug!(
                                "GetKeysClick -> Error propogating GetKeysResponse to callee -> {err:?}"
                            ),
                        }
                    });
                }

                ui.end_row();

                test_dropdown(
                    ui,
                    "ssd_cbox",
                    &mut self.ssd_test_cbox,
                    [SsdFail, SsdPass, SsdNotTested],
                );
                test_dropdown(
                    ui,
                    "hdd_cbox",
                    &mut self.hdd_test_cbox,
                    [HddFail, HddPass, HddNotTested],
                );
                test_dropdown(
                    ui,
                    "ram_cbox",
                    &mut self.ram_test_cbox,
                    [RamFail, RamPass, RamNotTested],
                );

                controls::toggle(ui, &mut self.send_specs, "Send System Info");

                ui.end_row();

                let mut attached_file = PathBuf::new();
                let dropped_files = ui.input_mut(|i| i.raw.take().dropped_files);
                for dropped_file in dropped_files {
                    self.opened_file = Some(dropped_file.path().to_path_buf());
                }

                if let Some(file) = &self.opened_file {
                    attached_file = file.to_path_buf();
                }

                // Extract just the file name from the PathBuf
                let _file_name = attached_file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");

                ui.label("");

                let is_windows = if cfg!(target_os = "windows") {
                    self.send_specs && !self.computer_data.cpu.is_empty()
                } else {
                    !self.computer_data.cpu.is_empty() && !self.computer_data.ram.is_empty()
                };

                let check = !self.ticket_data.service_number.is_empty()
                    && !self.customer_data.name.is_empty()
                    && !self.customer_data.phone_number.is_empty()
                    && !self.ticket_data.salesman.is_empty()
                    && !self.ticket_data.tech.is_empty()
                    && self.customer_data.phone_number != PLACEHOLDER_PHONE
                    && is_windows; // HIGHLIGHT ALL THE REQUIRED FIELDS IN RED

                let link_btn = TextButton::new("Create & Link Records")
                    .icon(svg::LINK)
                    .min_width(TICKET_COL_W)
                    .enabled(!self.ticket_data.service_number.is_empty())
                    .show(ui)
                    .on_hover_text(
                        "Creates and links customer, computer, and service order for remote diagnostics without creating or sending a task.",
                    );
                push_widget_anchor("tur.create_and_link_records", link_btn.rect);
                if link_btn.clicked() {
                    if self.shared_ctx.current_user.is_some() {
                        self.create_and_link_only();
                    } else {
                        let toast = &mut self.shared_ctx.toasts;
                        let error_toast = Toast {
                            kind: ToastKind::Error,
                            text: "You are not logged in".into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                            ..Default::default()
                        };
                        toast.add(error_toast);
                    }
                }

                let submit = TextButton::new("Submit TUR")
                    .icon(svg::PAPER_PLANE_TILT)
                    .kind(ButtonKind::Primary)
                    .min_width(TICKET_COL_W)
                    .enabled(check)
                    .show(ui);
                if submit.clicked() {
                    info!("Submitting TUR sheet");
                    if self.shared_ctx.current_user.is_some() {
                        self.submit_tur_mastertech();
                    } else {
                        let toast = &mut self.shared_ctx.toasts;
                        let error_toast = Toast {
                            kind: ToastKind::Error,
                            text: "You are not logged in".into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0),
                            ..Default::default()
                        };
                        toast.add(error_toast);
                    }
                }
                ui.end_row();
                ui.end_row();
            }); // grid
    }

    fn device_info_grid(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        Grid::new("Device Details Grid")
            .spacing(m.gap)
            .min_col_width(SPEC_COL_W)
            .max_col_width(SPEC_COL_W)
            .num_columns(2)
            .show(ui, |ui| {
                let service_details = &self.service_details;
                let mut device = service_details.first().cloned().unwrap_or_default();
                spec_field(ui, &mut device.device_name, svg::LAPTOP, "Device Name");
                spec_field(ui, &mut device.device_mfg, svg::BUILDINGS, "Device Mfg");
                ui.end_row();
                spec_field(ui, &mut device.device_model, svg::DEVICES, "Device Model");
                spec_field(ui, &mut device.device_serial, svg::BARCODE, "Device Serial");
                ui.end_row();
                spec_field(
                    ui,
                    &mut device.device_password,
                    svg::PASSWORD,
                    "Device Password",
                );
                spec_field(
                    ui,
                    &mut device.device_power_supply,
                    svg::PLUG,
                    "Device Power Supply",
                );
                ui.end_row();
            });
    }

    fn computer_info_grid(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        let t = Tokens::get(ui.ctx());
        Grid::new("Computer Specs")
            .spacing(m.gap)
            .min_col_width(SPEC_COL_W)
            .max_col_width(SPEC_COL_W)
            .num_columns(2)
            .show(ui, |ui| {
                let computer_data = &mut self.computer_data;
                let cpu_missing = computer_data.cpu.is_empty();

                TextField::new(&mut computer_data.cpu)
                    .hint("CPU")
                    .icon(svg::CPU)
                    .width(SPEC_COL_W)
                    .invalid(cpu_missing)
                    .show(ui);
                spec_field(ui, &mut computer_data.gpu, svg::GRAPHICS_CARD, "GPU");
                ui.end_row();
                spec_field(ui, &mut computer_data.ram, svg::MEMORY, "RAM");
                spec_field(
                    ui,
                    &mut computer_data.operating_system,
                    svg::WINDOWS_LOGO,
                    "OS",
                );
                ui.end_row();
                spec_field(
                    ui,
                    &mut computer_data.motherboard_name,
                    svg::CIRCUITRY,
                    "Motherboard Name",
                );
                spec_field(
                    ui,
                    &mut computer_data.motherboard_serial,
                    svg::BARCODE,
                    "Motherboard S/N",
                );
                ui.end_row();
                spec_field(
                    ui,
                    &mut computer_data.motherboard_asset_tag,
                    svg::TAG,
                    "Motherboard Asset Tag",
                );
                spec_field(
                    ui,
                    &mut computer_data.motherboard_vendor,
                    svg::BUILDING_OFFICE,
                    "Motherboard Vendor",
                );
                ui.end_row();
                spec_field(
                    ui,
                    &mut computer_data.product_name,
                    svg::PACKAGE,
                    "Product Name",
                );
                spec_field(
                    ui,
                    &mut computer_data.product_sku,
                    svg::BARCODE,
                    "Product SKU",
                );
                ui.end_row();
                spec_field(
                    ui,
                    &mut computer_data.product_serial,
                    svg::BARCODE,
                    "Product S/N",
                );
                spec_field(
                    ui,
                    &mut computer_data.product_vendor,
                    svg::STOREFRONT,
                    "Product Vendor",
                );
                ui.end_row();
                spec_field(ui, &mut computer_data.hostname, svg::DESKTOP, "Hostname");

                // Laptops only; desktops report no battery and get no row.
                if let Some(battery) = computer_data.battery.clone() {
                    let health = battery.health_percent();
                    let (icon, color) = match health {
                        Some(pct) if pct < 60 => (svg::BATTERY_WARNING, t.error),
                        Some(pct) if pct < 80 => (svg::BATTERY_MEDIUM, t.warn),
                        _ => (svg::BATTERY_FULL, t.text),
                    };
                    ui.horizontal(|ui| {
                        ui.add(icon.image(m.icon_md).tint(color));
                        ui.colored_label(color, format!("Battery: {}", battery.summary()))
                    })
                    .inner
                    .on_hover_text(if battery.name.is_empty() {
                        "Full-charge capacity against the pack's design capacity".to_string()
                    } else {
                        battery.name.clone()
                    });
                }

                ui.end_row();
            });
    }

    fn product_info_grid(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        if self.order_rows.is_empty() {
            controls::icon_label(
                ui,
                svg::PACKAGE,
                "No products yet. Pull an order to list them.",
            );
            return;
        }
        Grid::new("Product Detail Grid")
            .spacing(m.gap)
            .min_col_width(SPEC_COL_W)
            .max_col_width(SPEC_COL_W)
            .num_columns(2)
            .show(ui, |ui| {
                controls::section_label(ui, "Product");
                controls::section_label(ui, "Price");
                ui.end_row();

                for item in self.order_rows.iter() {
                    ui.label(format!(
                        "{} (x{})",
                        item.product_name.clone(),
                        item.product_quantity.clone()
                    ));
                    match item.product_price.clone().parse::<f64>() {
                        Ok(price_num) => ui.label(format!("${:.2}", price_num)),
                        Err(_) => ui.label(format!("${}", item.product_price.clone())),
                    };
                    ui.end_row();
                }
            });
    }

    fn seb_info_grid(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        let seb_info = &self.seb_info;
        let mut seb_details = seb_info.first().cloned().unwrap_or_default();
        let full = ui.available_width();

        TextField::new(&mut seb_details.device_name)
            .hint("Carbonite Device Name")
            .icon(svg::CLOUD)
            .width(full)
            .show(ui);

        let code = if !seb_details.activation_code.is_empty() {
            seb_details.activation_code.to_uppercase()
        } else {
            "SEB Code".to_string()
        };
        if TextButton::new(&code)
            .icon(svg::COPY)
            .min_width(full)
            .show(ui)
            .on_hover_text("Click To Copy SEB Code to Clipboard")
            .clicked()
        {
            ui.ctx()
                .copy_text(seb_details.activation_code.to_uppercase());
        };

        let id = if !seb_details.device_id.is_empty() {
            seb_details.device_id.to_uppercase()
        } else {
            "Device ID".to_string()
        };
        if TextButton::new(&id)
            .icon(svg::COPY)
            .min_width(full)
            .show(ui)
            .on_hover_text("Click To Copy Device ID to Clipboard")
            .clicked()
        {
            ui.ctx().copy_text(seb_details.device_id.to_uppercase());
        };
        ui.add_space(m.gap.y);

        Grid::new("SEB Info Grid")
            .spacing(m.gap)
            .min_col_width(SPEC_COL_W)
            .max_col_width(SPEC_COL_W)
            .num_columns(2)
            .show(ui, |ui| {
                spec_field(
                    ui,
                    &mut seb_details.activated,
                    svg::CALENDAR_CHECK,
                    "Date Activated",
                );
                spec_field(
                    ui,
                    &mut seb_details.current_period_ends_at,
                    svg::CALENDAR_X,
                    "Current Period Ends At",
                );
                ui.end_row();
                spec_field(
                    ui,
                    &mut seb_details.id_recurly_account,
                    svg::IDENTIFICATION_CARD,
                    "Recurly Id",
                );
                let mut usage = format!("{} Gb", seb_details.usage_gb);
                spec_field(ui, &mut usage, svg::DATABASE, "Usage (Gb)");
                ui.end_row();
            }); // grid
    }

    fn recommendations_grid(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        Grid::new("RecommendationsCheckinNotesGrid")
            .spacing(m.gap)
            .min_col_width(NOTES_COL_W)
            .max_col_width(NOTES_COL_W)
            .num_columns(2)
            .show(ui, |ui| {
                let tur_checkin = TextArea::new(&mut self.ticket_data.checkin_notes)
                    .hint("Checkin Notes")
                    .mono(true)
                    .width(NOTES_COL_W)
                    .rows(NOTES_ROWS)
                    .show(ui);
                push_widget_anchor("tur.checkin_notes", tur_checkin.rect);

                let recommendations_check = self.task_data.task_description.is_empty();

                let tur_rec = TextArea::new(&mut self.task_data.task_description)
                    .hint("Recommendations")
                    .mono(true)
                    .width(NOTES_COL_W)
                    .rows(NOTES_ROWS)
                    .invalid(recommendations_check)
                    .show(ui);
                push_widget_anchor("tur.recommendations", tur_rec.rect);
            });
    }
}

/// A hardware-test result picker over `options`.
fn test_dropdown(ui: &mut Ui, salt: &str, value: &mut HardwareTest, options: [HardwareTest; 3]) {
    let options: Vec<(HardwareTest, &str)> = options
        .into_iter()
        .map(|test| {
            let label = test.as_str();
            (test, label)
        })
        .collect();
    controls::dropdown(ui, salt, value, &options, TICKET_COL_W);
}

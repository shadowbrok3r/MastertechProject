use crate::ui_tools::controls::IconLabel;
use database::{schema::{odoo::{search_odoo_products, ExtraInventoryData}, prestashop::{Address, Customer, DesktopModel, Device, DeviceMfg, LaptopModel, Order, PrestashopOrderType, ServiceOrder}, RecordId, User, assignable_users}, PlatformSpawner, Spawner};
use eframe::egui::{Align, CentralPanel, Frame, Grid, Layout, Panel, Rect, RichText, ScrollArea, Ui};
use crate::{get_current_user_from_auth, get_database_users, modals::tabs::return_colors};
use crate::ui_tools::controls::{self, BarKind, Metrics, TextField};
use crate::ui_tools::icons::svg::{self};
use crate::ui_tools::{glass_card, theme};
use crossbeam::channel::{Receiver, Sender};
use web_time::{Duration, Instant};

/// Width of each picker in the Device Details card.
const DEVICE_PICKER_W: f32 = 104.0;
/// Width of the rep and order-type pickers.
const REP_PICKER_W: f32 = 160.0;

pub struct PrestashopOrderForm {
    order: Order,
    customer: Customer,
    address: Address,
    _service_order: ServiceOrder,
    service_details: ServiceDetails,
    state: UiState,
    data: Vec<(Customer, Address)>,
    
    odoo_product_search: String,
    searched_products: Vec<ExtraInventoryData>,
    added_products: Vec<ExtraInventoryData>,
    last_search_time: Instant,
    store_users: Vec<User>,
    current_user: User,
    order_state: PrestashopOrderType,
    first_ui_run: bool,
    sales_rep: User,
    add_split_rep: bool,
    split_rep: User,
    serial_search: String,
    /// Why the last search returned nothing selectable. Empty when it did.
    search_status: String,

    action_tx: Sender<UiAction>,
    action_rx: Receiver<UiAction>,
    search_results_tx: Sender<Vec<(Customer, Address)>>,
    search_results_rx: Receiver<Vec<(Customer, Address)>>,
    status_tx: Sender<String>,
    status_rx: Receiver<String>,
    odoo_search_tx: Sender<Vec<ExtraInventoryData>>,
    odoo_search_rx: Receiver<Vec<ExtraInventoryData>>,
}

#[derive(Default)]
pub struct ServiceDetails {
    device_mfg: DeviceMfg,
    device: Device,
    device_laptop: LaptopModel,
    device_desktop: DesktopModel,
    device_pw: String,
    device_serial: String,
    power_cord: bool,
    power_cord_serial: String,
    checkin_notes: String,
}

pub enum UiState {
    SelectCustomer,
    CreateCustomer,
    CreateOrder,
}

pub enum UiAction {
    SearchCustomerEmail(String),
    SearchCustomerPhone(String),
    SearchCustomerSerial(String),
    SetSelectedCustomer((Customer, Address)),
    SearchProduct(String),
    AddProduct(ExtraInventoryData),
    RemoveProduct(ExtraInventoryData),
    // SetSelectedCustomer(Customer),
    NewState(UiState)
}

impl PrestashopOrderForm {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let (search_results_tx, search_results_rx) = crossbeam::channel::unbounded();
        let (status_tx, status_rx) = crossbeam::channel::unbounded();
        let (odoo_search_tx, odoo_search_rx) = crossbeam::channel::unbounded();

        Self {
            order: Default::default(),
            customer: Default::default(),
            address: Default::default(),
            _service_order: Default::default(),
            service_details: ServiceDetails::default(),
            odoo_product_search: String::new(),
            searched_products: vec![],
            added_products: vec![],
            state: UiState::SelectCustomer,
            order_state: PrestashopOrderType::default(),
            data: vec![],
            first_ui_run: true,
            sales_rep: User::default(),
            split_rep: User::default(),
            add_split_rep: false,
            serial_search: String::new(),
            search_status: String::new(),

            action_tx, action_rx,
            search_results_tx, search_results_rx,
            status_tx, status_rx,
            odoo_search_tx, odoo_search_rx,
            last_search_time: Instant::now(),
            store_users: vec![],
            current_user: User::default(),
        }
    }

    pub fn receive(&mut self, ui: &mut Ui) {
        if let Ok(action) = self.action_rx.try_recv() {
            ui.ctx().request_repaint();
            match action {
                UiAction::NewState(state) => { self.state = state; },
                UiAction::SearchCustomerEmail(email) => {
                    let search_email = email.clone();
                    let tx = self.search_results_tx.clone();
                    if !search_email.is_empty() {
                        PlatformSpawner::spawn(async move {
                            match Customer::find_customer_by_email(&search_email).await {
                                Ok(customers) => { let _ = tx.try_send(customers); },
                                Err(e) => log::error!("Error Pulling customer info via email: {e:?}"),
                            }
                        });
                    }
                },
                UiAction::SearchCustomerPhone(phone) => {
                    let search_phone = phone.clone();
                    let tx = self.search_results_tx.clone();
                    if !search_phone.is_empty() {
                        PlatformSpawner::spawn(async move {
                            match Customer::find_customer_by_phone(&search_phone).await {
                                Ok(customers) => { let _ = tx.try_send(customers); },
                                Err(e) => log::error!("Error Pulling customer info via email: {e:?}"),
                            }
                        });
                    }
                },
                UiAction::SearchCustomerSerial(serial) => {
                    let search_serial = serial.trim().to_string();
                    let tx = self.search_results_tx.clone();
                    let status_tx = self.status_tx.clone();
                    if !search_serial.is_empty() {
                        self.search_status = format!("Searching for serial {search_serial}...");
                        PlatformSpawner::spawn(async move {
                            match serial_customer_search(&search_serial).await {
                                Ok((customers, status)) => {
                                    let _ = status_tx.try_send(status);
                                    let _ = tx.try_send(customers);
                                },
                                Err(e) => {
                                    log::error!("Error looking up customer by serial: {e:?}");
                                    let _ = status_tx.try_send(format!("Serial lookup failed: {e}"));
                                    let _ = tx.try_send(vec![]);
                                },
                            }
                        });
                    }
                },
                UiAction::SetSelectedCustomer((customer, address)) => {
                    self.customer = customer;
                    self.address = address;
                    self.state = UiState::CreateOrder;
                },
                UiAction::SearchProduct(search_term) => {
                    let search = search_term.clone();
                    let tx = self.odoo_search_tx.clone();
                    PlatformSpawner::spawn(async move {
                        match search_odoo_products(&search).await {
                            Ok(products) => { let _ = tx.try_send(products); },
                            Err(e) => log::error!("Error with searching odoo products: {e:?}"),
                        }
                    });
                }
                UiAction::AddProduct(product) => {
                    self.added_products.push(product);
                },
                UiAction::RemoveProduct(product) => {
                    self.added_products.retain(|p| p.product_variant_id.0 != product.product_variant_id.0);
                }
            }
        }
    
        if let Ok(products) = self.odoo_search_rx.try_recv() {
            ui.ctx().request_repaint();
            self.searched_products = products.clone();
        }

        if let Ok(data) = self.search_results_rx.try_recv() {
            ui.ctx().request_repaint();
            self.data = data;
        }

        if let Ok(status) = self.status_rx.try_recv() {
            ui.ctx().request_repaint();
            self.search_status = status;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive(ui);
        let m = Metrics::get(ui.ctx());

        Panel::top("PrestashopOrderTopPanel")
        .exact_size(m.bar_h)
        .frame(Frame::NONE.fill(ui.visuals().panel_fill))
        .show_separator_line(false)
        .show(ui, |ui|{
            controls::bar(ui, BarKind::Tool, |ui| {
                if controls::icon_text_button(ui, svg::PLUS, "New Order").clicked() {
                    let _ = self.action_tx.try_send(UiAction::NewState(UiState::SelectCustomer));
                }

                if controls::icon_text_button(ui, svg::USER_PLUS, "Create Customer").clicked() {
                    let _ = self.action_tx.try_send(UiAction::NewState(UiState::CreateCustomer));
                }

                match self.state {
                    UiState::CreateOrder => {
                        controls::vline(ui, m.control_h);
                        ui.label(RichText::new("Creating Order for").color(theme::weak_text(ui)));
                        let name = format!("{} {}", self.customer.firstname, self.customer.lastname);
                        if controls::pill(ui, svg::USER, &name, true)
                            .on_hover_text("Change customer")
                            .clicked()
                        {
                            let _ = self.action_tx.try_send(UiAction::NewState(UiState::SelectCustomer));
                        }
                    },
                    UiState::SelectCustomer => {
                        controls::vline(ui, m.control_h);
                        TextField::new(&mut self.customer.email)
                            .hint("Customer Email")
                            .icon(svg::ENVELOPE_SIMPLE)
                            .width(200.)
                            .show(ui);

                        TextField::new(&mut self.address.phone)
                            .hint("Customer Phone #")
                            .icon(svg::PHONE)
                            .width(160.)
                            .show(ui);

                        TextField::new(&mut self.serial_search)
                            .hint("Serial #")
                            .icon(svg::BARCODE)
                            .width(160.)
                            .show(ui);

                        let search_btn = controls::icon_text_button(ui, svg::MAGNIFYING_GLASS, "Search");

                        // Serial first: it is scanned, so it is the most specific
                        // of the three and the least likely to be left over.
                        if search_btn.clicked() {
                            self.search_status.clear();
                            if !self.serial_search.trim().is_empty() {
                                let _ = self.action_tx.try_send(UiAction::SearchCustomerSerial(self.serial_search.clone()));
                            } else if !self.customer.email.is_empty() {
                                let _ = self.action_tx.try_send(UiAction::SearchCustomerEmail(self.customer.email.clone()));
                            } else if !self.address.phone.is_empty() {
                                let _ = self.action_tx.try_send(UiAction::SearchCustomerPhone(self.address.phone.clone()));
                            }
                        }
                    },
                    _ => {}
                }
            });
        });

        CentralPanel::default()
        .show(ui, |ui|{
            ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                match self.state {
                    UiState::SelectCustomer => self.select_customer(ui),
                    UiState::CreateCustomer => self.create_customer(ui),
                    UiState::CreateOrder => self.create_order(ui),
                }
            });
        });
    }

    // 8013914625
    pub fn select_customer(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        ui.add_space(m.gap.y);

        if !self.search_status.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let warn = theme::warn(ui);
                ui.add(svg::WARNING.image(m.icon_sm).tint(warn));
                ui.colored_label(warn, &self.search_status);
            });
            ui.add_space(m.gap.y);
        }

        glass_card::group(ui, |ui| {
            glass_card::card_header(ui, svg::USERS, "Customers", None);
            if self.data.is_empty() {
                controls::empty_state(
                    ui,
                    svg::USERS,
                    "No customers to show",
                    "Search by email, phone or serial above.",
                    &[],
                );
                return;
            }
            Grid::new("Customer Selection Grid")
            .num_columns(6)
            .spacing(m.gap)
            .min_col_width(ui.available_width()/6.1)
            .max_col_width(ui.available_width()/6.1)
            .striped(true)
            .with_row_color(return_colors)
            .show(ui, |ui| {
                controls::section_label(ui, "First Name");
                controls::section_label(ui, "Last Name");
                controls::section_label(ui, "Phone #");
                controls::section_label(ui, "Email");
                controls::section_label(ui, "Address");
                ui.label("");
                ui.end_row();

                for (customer, address) in self.data.iter() {
                    ui.label(&customer.firstname);
                    ui.label(&customer.lastname);
                    ui.label(&address.phone);
                    ui.label(&customer.email);
                    ui.label(&address.address1);
                    if controls::icon_text_button(ui, svg::USER_CHECK, "Select").clicked() {
                        let _ = self.action_tx.try_send(UiAction::SetSelectedCustomer((customer.clone(), address.clone())));
                    }
                    ui.end_row();
                }
            });
        });
    }

    pub fn create_customer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui|{
            let email_search = TextField::new(&mut self.customer.email)
            .hint("Customer Email")
            .icon(svg::ENVELOPE_SIMPLE)
            .width(220.)
            .show(ui);

            if email_search.lost_focus() && !self.customer.email.is_empty() {
                let _ = self.action_tx.try_send(UiAction::SearchCustomerEmail(self.customer.email.clone()));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let phone_search = TextField::new(&mut self.address.phone)
                .hint("Customer Phone #")
                .icon(svg::PHONE)
                .width(220.)
                .show(ui);

                if phone_search.lost_focus() && !self.address.phone.is_empty() {
                    let _ = self.action_tx.try_send(UiAction::SearchCustomerPhone(self.address.phone.clone()));
                }
            });
        });
    }

    pub fn create_order(&mut self, ui: &mut Ui) {
        if self.first_ui_run {
            log::error!("User email is empty: {:?}", self.current_user.get_email().is_empty());
            self.current_user = get_current_user_from_auth().unwrap_or_default();
            self.sales_rep = self.current_user.clone();
            self.split_rep = self.current_user.clone();
            self.store_users = get_database_users();
            self.first_ui_run = false;
        }

        let m = Metrics::get(ui.ctx());
        ui.add_space(m.gap.y);

        #[allow(const_item_mutation)]
        let product_search_rect = &mut Rect::ZERO;

        ui.columns(2, |ui| {
            ui[0].vertical_centered(|ui| {
                ui.horizontal_top(|ui| {
                    glass_card::group(ui, |ui| {
                        glass_card::card_header(ui, svg::USER, "Customer", None);
                        Grid::new("Selected Customer Grid")
                        .num_columns(2)
                        .spacing(m.gap)
                        .min_col_width(180.)
                        .max_col_width(180.)
                        .striped(true)
                        .with_row_color(return_colors)
                        .show(ui, |ui| {
                            let customer = &self.customer;
                            let address = &self.address;
                            IconLabel::new(svg::IDENTIFICATION_CARD, "ID").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&customer.id);
                            ui.end_row();
                            IconLabel::new(svg::USER, "Name").tone(theme::weak_text(ui)).show(ui);
                            ui.label(format!("{} {}", customer.firstname, customer.lastname));
                            ui.end_row();
                            IconLabel::new(svg::PHONE, "Phone #").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&address.phone);
                            ui.end_row();
                            IconLabel::new(svg::DEVICE_MOBILE, "Mobile #").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&address.phone_mobile);
                            ui.end_row();
                            IconLabel::new(svg::ENVELOPE_SIMPLE, "Email").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&customer.email);
                            ui.end_row();
                            IconLabel::new(svg::BUILDINGS, "City").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&address.city);
                            ui.end_row();
                            IconLabel::new(svg::MAP_PIN, "Address").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&address.address1);
                            ui.end_row();
                            IconLabel::new(svg::MAP_PIN, "Address 2").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&address.address2);
                            ui.end_row();
                            IconLabel::new(svg::HASH, "Zipcode").tone(theme::weak_text(ui)).show(ui);
                            ui.label(&address.postcode);
                            ui.end_row();
                        });
                    });

                    ui.add_space(2.0 * m.gap.x);

                    ui.vertical_centered(|ui| {
                        ui.set_width(350.);
                        glass_card::group(ui, |ui| {
                            glass_card::card_header(ui, svg::LAPTOP, "Device Details", None);
                            ui.add_space(m.gap.y);

                            ui.horizontal(|ui| {
                                let devices = Device::VALUES;
                                let device_options: Vec<(Device, &str)> =
                                    devices.iter().map(|d| (d.clone(), d.as_str())).collect();
                                controls::dropdown(ui, "Device Selection", &mut self.service_details.device, &device_options, DEVICE_PICKER_W);

                                let makers = DeviceMfg::VALUES;
                                let maker_options: Vec<(DeviceMfg, &str)> =
                                    makers.iter().map(|d| (d.clone(), d.as_str())).collect();
                                controls::dropdown(ui, "Device Mfg Selection", &mut self.service_details.device_mfg, &maker_options, DEVICE_PICKER_W);

                                if self.service_details.device_mfg == DeviceMfg::PcLaptops {
                                    if self.service_details.device == Device::Desktop {
                                        let models = DesktopModel::VALUES;
                                        let model_options: Vec<(DesktopModel, &str)> =
                                            models.iter().map(|d| (d.clone(), d.as_str())).collect();
                                        controls::dropdown(ui, "Desktop Selection", &mut self.service_details.device_desktop, &model_options, DEVICE_PICKER_W);
                                    } else if self.service_details.device == Device::Laptop {
                                        let models = LaptopModel::VALUES;
                                        let model_options: Vec<(LaptopModel, &str)> =
                                            models.iter().map(|d| (d.clone(), d.as_str())).collect();
                                        controls::dropdown(ui, "Laptop Selection", &mut self.service_details.device_laptop, &model_options, DEVICE_PICKER_W);
                                    }
                                }
                            });

                            ui.add_space(m.gap.y);
                            TextField::new(&mut self.service_details.device_pw)
                                .hint("Device Password")
                                .icon(svg::PASSWORD)
                                .width(ui.available_width())
                                .show(ui);
                            ui.add_space(m.gap.y);
                            TextField::new(&mut self.service_details.device_serial)
                                .hint("Device Serial")
                                .icon(svg::BARCODE)
                                .width(ui.available_width())
                                .show(ui);
                            ui.add_space(m.gap.y);
                            ui.horizontal(|ui| {
                                controls::toggle(ui, &mut self.service_details.power_cord, "Power Cord?");
                                if self.service_details.power_cord {
                                    TextField::new(&mut self.service_details.power_cord_serial)
                                        .hint("Power Cord Serial")
                                        .icon(svg::PLUG)
                                        .width(150.)
                                        .show(ui);
                                }
                            });
                            ui.add_space(m.gap.y);
                            controls::TextArea::new(&mut self.service_details.checkin_notes).hint("Check-In Notes").rows(6).show(ui);
                        });
                    });
                });

                ui.add_space(2.0 * m.gap.y);

                let res = controls::search_field(ui, &mut self.odoo_product_search, "Search for a product", 220.);

                if res.has_focus()
                    && res.changed()
                    && self.odoo_product_search.len() > 1
                    && self.last_search_time.elapsed() > Duration::from_millis(50)
                {
                    self.last_search_time = Instant::now();
                    let _ = self.action_tx.try_send(UiAction::SearchProduct(self.odoo_product_search.clone()));
                }

                ui.add_space(m.gap.y);

                *product_search_rect = glass_card::group(ui, |ui| {
                    glass_card::card_header(ui, svg::MAGNIFYING_GLASS, "Product Search Results", None);

                    let products: Vec<&ExtraInventoryData> = self
                        .searched_products
                        .iter()
                        .filter(|p|
                            !p.default_code
                            .as_deref()
                            .is_some_and(|c| c.to_lowercase().ends_with("/xidax"))
                        )
                        .collect();
                    if products.is_empty() {
                        controls::empty_state(ui, svg::PACKAGE, "No products", "Search for a product above.", &[]);
                        return;
                    }

                    Grid::new("Product Selection Grid")
                    .num_columns(5)
                    .spacing(m.gap)
                    .min_col_width(ui.available_width()/5.1)
                    .max_col_width(ui.available_width()/5.1)
                    .striped(true)
                    .with_row_color(return_colors)
                    .show(ui, |ui| {
                        controls::section_label(ui, "Product Code");
                        controls::section_label(ui, "Qty Avail");
                        controls::section_label(ui, "Cost");
                        controls::section_label(ui, "Standard Price");
                        ui.label("");
                        ui.end_row();

                        for product in products {
                            ui.label(product.default_code.as_deref().unwrap_or(&product.name));
                            ui.label(product.qty_available.to_string());
                            ui.label(format!(" $ {:.2}", product.list_price));
                            ui.label(format!(" $ {:.2}", product.standard_price));
                            if controls::icon_text_button(ui, svg::PLUS, "Add").clicked() {
                                let _ = self.action_tx.try_send(UiAction::AddProduct(product.clone()));
                            }
                            ui.end_row();
                        }
                    });
                }).response.rect;
            });

            ui[1].vertical_centered(|ui| {
                ui.vertical_centered(|ui| {
                    ui.set_width(435.);
                    glass_card::group(ui, |ui| {
                        glass_card::card_header(ui, svg::CLIPBOARD_TEXT, "Service Details", None);
                        ui.add_space(m.gap.y);

                        let sorted_users = assignable_users(&self.store_users, self.sales_rep.get_store());
                        let user_options: Vec<(RecordId, &str)> = sorted_users
                            .iter()
                            .map(|u| (u.get_id(), u.get_username()))
                            .collect();

                        Grid::new("Service Details Grid")
                        .num_columns(2)
                        .spacing(m.gap)
                        .show(ui, |ui| {
                            IconLabel::new(svg::USER, "Sales Rep").tone(theme::weak_text(ui)).show(ui);
                            let mut rep_id = self.sales_rep.get_id();
                            if controls::dropdown(ui, "Sales Rep Selection", &mut rep_id, &user_options, REP_PICKER_W).changed()
                                && let Some(user) = sorted_users.iter().copied().find(|u| u.get_id() == rep_id)
                            {
                                self.sales_rep = user.clone();
                            }
                            ui.end_row();

                            controls::toggle(ui, &mut self.add_split_rep, "Split Rep");
                            if self.add_split_rep {
                                let split_options: Vec<(RecordId, &str)> = sorted_users
                                    .iter()
                                    .filter(|u| u.get_id() != self.sales_rep.get_id())
                                    .map(|u| (u.get_id(), u.get_username()))
                                    .collect();
                                let mut split_id = self.split_rep.get_id();
                                if controls::dropdown(ui, "Split Rep Selection", &mut split_id, &split_options, REP_PICKER_W).changed()
                                    && let Some(user) = sorted_users.iter().copied().find(|u| u.get_id() == split_id)
                                {
                                    self.split_rep = user.clone();
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();

                            IconLabel::new(svg::TAG, "Order Type").tone(theme::weak_text(ui)).show(ui);
                            let order_types = PrestashopOrderType::VALUES;
                            let type_options: Vec<(PrestashopOrderType, &str)> = order_types
                                .iter()
                                .map(|t| (t.clone(), t.as_str()))
                                .collect();
                            controls::dropdown(ui, "Order State", &mut self.order_state, &type_options, REP_PICKER_W);
                            ui.end_row();
                        });
                    });
                });

                // Calculate the space needed to align the "Added Products" group with the "Product Search Results" group
                let target_y = product_search_rect.min.y; // Top y-coordinate of "Product Search Results"
                let current_y = ui.cursor().min.y; // Current y-position in the right column
                let space_to_add = target_y - current_y; // Space needed to align tops

                if space_to_add > 0.0 {
                    ui.add_space(space_to_add); // Add space to push "Added Products" down
                }

                glass_card::group(ui, |ui| {
                    glass_card::card_header(ui, svg::SHOPPING_CART, "Added Products", None);
                    if self.added_products.is_empty() {
                        controls::empty_state(
                            ui,
                            svg::SHOPPING_CART,
                            "No products added",
                            "Add products from the search results.",
                            &[],
                        );
                        return;
                    }

                    Grid::new("Added Products Grid")
                    .num_columns(5)
                    .spacing(m.gap)
                    .min_col_width(ui.available_width()/5.1)
                    .max_col_width(ui.available_width()/5.1)
                    .striped(true)
                    .with_row_color(return_colors)
                    .show(ui, |ui| {
                        controls::section_label(ui, "Product Code");
                        controls::section_label(ui, "Qty Avail");
                        controls::section_label(ui, "Cost");
                        controls::section_label(ui, "Standard Price");
                        ui.label("");
                        ui.end_row();

                        let total = &mut 0.0;
                        for product in self.added_products.iter() {
                            *total += product.list_price;
                            ui.label(product.default_code.as_deref().unwrap_or(&product.name));
                            ui.label(product.qty_available.to_string());
                            ui.label(format!(" $ {:.2}", product.list_price));
                            ui.label(format!(" $ {:.2}", product.standard_price));
                            if controls::icon_text_button(ui, svg::MINUS_CIRCLE, "Remove").clicked() {
                                let _ = self.action_tx.try_send(UiAction::RemoveProduct(product.clone()));
                            }
                            ui.end_row();
                        }

                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label(
                            RichText::new(format!("Subtotal: $ {:.2}", total))
                                .strong()
                                .color(theme::primary(ui)),
                        );

                        ui.end_row();
                    });
                });
            });
        });
    }

    pub fn submit_order(&mut self) -> anyhow::Result<(), anyhow::Error> {
        self.order = Order::default(); //{
        //     id_order_type: todo!(),
        //     id_address_delivery: todo!(),
        //     id_address_invoice: todo!(),
        //     id_customer: todo!(),
        //     current_state: todo!(),
        //     invoice_number: todo!(),
        //     invoice_date: todo!(),
        //     payment: todo!(),
        //     date_add: todo!(),
        //     date_upd: todo!(),
        //     id_employee_sales_rep: todo!(),
        //     id_employee_split_rep: todo!(),
        //     id_employee_editing: todo!(),
        //     id_order_everest: todo!(),
        //     id_store: todo!(),
        //     total_paid: todo!(),
        //     delivery_date: todo!(),
        //     total_products_wt: todo!(),
        //     total_paid_tax_excl: todo!(),
        //     reference: todo!(),
        //     id_order_parent: todo!(),
        //     shipping_number: todo!(),
        //     order_type: todo!(),
        //     associations: todo!(),
        // };

        Ok(())
    }
}

/* // Required Fields
// Use quickxml_to_serde
{
  "order": {
    "id_address_delivery": "175741",
    "id_address_invoice": "225960",
    "id_cart": "5327739",
    "id_currency": "1",
    "id_lang": "1",
    "id_customer": "22569",
    "id_carrier": "255",
    "module": "ps_creditcard",
    "payment": "Credit Card (manual)",
    "total_products": "99.980000",
    "total_products_wt": "107.420000",
    "conversion_rate": "1.000000",
    "associations": {
      "order_rows": [
        {
          "product_id": "16418",
          "product_attribute_id": "0",
          "product_quantity": "1"
        },
        {
          "product_id": "16418",
          "product_attribute_id": "0",
          "product_quantity": "1"
        }
      ]
    }
  }
}

*/
/// Customers who own an order carrying `serial`, plus a line explaining an
/// empty result.
///
/// Only PrestaShop customers are selectable here, because that is the only id
/// space the create-order form writes. A Shopify-only serial therefore returns
/// no rows — the status line names the order it did find so the search does not
/// read as "unknown serial".
async fn serial_customer_search(
    serial: &str,
) -> anyhow::Result<(Vec<(Customer, Address)>, String)> {
    let presta = Customer::find_customer_by_serial(serial).await;
    match &presta {
        Ok(pairs) if !pairs.is_empty() => return Ok((pairs.clone(), String::new())),
        _ => {}
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        match database::orders::serial_lookup::lookup(serial).await {
            Ok(Some(hit)) => {
                let order = hit
                    .orders
                    .first()
                    .map(|o| o.reference.clone())
                    .unwrap_or_else(|| "an order".into());
                let who = if hit.name.trim().is_empty() { hit.email.clone() } else { hit.name.clone() };
                return Ok((
                    vec![],
                    format!(
                        "Serial {serial} is on {} order {order} for {who}. \
                         This form writes PrestaShop customers, so it cannot be selected here yet.",
                        hit.source
                    ),
                ));
            }
            Ok(None) => {}
            Err(e) => log::warn!("serial_customer_search -> backend lookup failed: {e:#}"),
        }
    }

    // A PrestaShop error and a PrestaShop miss are different answers, and only
    // the miss means the serial is unknown.
    match presta {
        Ok(_) => Ok((vec![], format!("No order carries serial {serial}."))),
        Err(e) => Err(e),
    }
}

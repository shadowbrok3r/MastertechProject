use web_time::{Duration, Instant};
use crossbeam::channel::{Receiver, Sender};
use database::{schema::{odoo::{search_odoo_products, ExtraInventoryData}, prestashop::{Address, Customer, DesktopModel, Device, DeviceMfg, LaptopModel, Order, ServiceOrder}, CustomerData}, PlatformSpawner, Spawner};
use eframe::egui::{Align, CentralPanel, ComboBox, Direction, FontId, Grid, Id, Layout, RichText, ScrollArea, TextEdit, TopBottomPanel, Ui, UiBuilder, Widget};

use crate::modals::tabs::return_colors;



pub struct PrestashopOrderForm {
    order: Order,
    customer: Customer,
    address: Address,
    service_order: ServiceOrder,
    service_details: ServiceDetails,
    state: UiState,
    data: Vec<(Customer, Address)>,
    
    odoo_product_search: String,
    searched_products: Vec<ExtraInventoryData>,
    added_products: Vec<ExtraInventoryData>,
    last_search_time: Instant,

    action_tx: Sender<UiAction>,
    action_rx: Receiver<UiAction>,
    search_results_tx: Sender<Vec<(Customer, Address)>>,
    search_results_rx: Receiver<Vec<(Customer, Address)>>,
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
    SetSelectedCustomer(Customer),
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
        let (odoo_search_tx, odoo_search_rx) = crossbeam::channel::unbounded();

        Self {
            order: Default::default(),
            customer: Default::default(),
            address: Default::default(),
            service_order: Default::default(),
            service_details: ServiceDetails::default(),
            odoo_product_search: String::new(),
            searched_products: vec![],
            added_products: vec![],
            state: UiState::SelectCustomer,
            data: vec![],
            action_tx, action_rx,
            search_results_tx, search_results_rx,
            odoo_search_tx, odoo_search_rx,
            last_search_time: Instant::now(),
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
                UiAction::SetSelectedCustomer(customer) => {
                    self.customer = customer;
                    // self.address = 
                    self.state = UiState::CreateOrder;
                },
                UiAction::SearchProduct(search_term) => {
                    let search = search_term.clone();
                    let tx = self.odoo_search_tx.clone();
                    PlatformSpawner::spawn(async move {
                        match search_odoo_products(&search).await {
                            Ok(products) => { let _ = tx.try_send(products.result); },
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
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive(ui);

        TopBottomPanel::top("PrestashopOrderTopPanel")
        .exact_height(25.)
        .show_inside(ui, |ui|{
            ui.horizontal_top(|ui| {
                ui.add_space(5.);
                if ui.button("New Order +").clicked() {
                    let _ = self.action_tx.try_send(UiAction::NewState(UiState::SelectCustomer));
                }
                
                ui.add_space(5.);
                if ui.button("Create Customer +").clicked() {
                    let _ = self.action_tx.try_send(UiAction::NewState(UiState::CreateCustomer));
                }
                
                ui.add_space(5.);

                match self.state {
                    UiState::CreateOrder => {
                        ui.label(format!("Creating Order for {} {}", self.customer.firstname, self.customer.lastname));
                        ui.add_space(5.);
                        if ui.button("Change Customer").clicked() {
                            let _ = self.action_tx.try_send(UiAction::NewState(UiState::SelectCustomer));
                        }
                    },
                    _ => {}
                }
            });
        });

        CentralPanel::default()
        .show_inside(ui, |ui|{
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
        ui.horizontal(|ui|{
            TextEdit::singleline(&mut self.customer.email)
            .hint_text("Customer Email")
            .ui(ui);

            ui.add_space(10.);
            
            TextEdit::singleline(&mut self.address.phone)
            .hint_text("Customer Phone #")
            .ui(ui);

            ui.add_space(10.);

            let search_btn = ui.button("Search");

            if search_btn.clicked() {
                if !self.customer.email.is_empty() {
                    let _ = self.action_tx.try_send(UiAction::SearchCustomerEmail(self.customer.email.clone()));
                } else if !self.address.phone.is_empty() {
                    let _ = self.action_tx.try_send(UiAction::SearchCustomerPhone(self.address.phone.clone()));
                }
            }
        });

        Grid::new("Customer Selection Grid")
        .num_columns(6)
        .min_col_width(ui.available_width()/6.)
        .max_col_width(ui.available_width()/6.)
        .striped(true)
        .with_row_color(return_colors)
        .show(ui, |ui| {
            ui.colored_label(ui.style().visuals.error_fg_color, "First Name");
            ui.colored_label(ui.style().visuals.error_fg_color, "Last Name");
            ui.colored_label(ui.style().visuals.error_fg_color, "Phone #");
            ui.colored_label(ui.style().visuals.error_fg_color, "Email");
            ui.colored_label(ui.style().visuals.error_fg_color, "Address");
            ui.label("");
            ui.end_row();

            for (customer, address) in self.data.iter() {
                ui.label(&customer.firstname);
                ui.label(&customer.lastname);
                ui.label(&address.phone);
                ui.label(&customer.email);
                ui.label(&address.address1);
                if ui.button("Select").clicked() {
                    let _ = self.action_tx.try_send(UiAction::SetSelectedCustomer(customer.clone()));
                }
                ui.end_row();
            }
            
            ui.end_row();
        });
    }

    pub fn create_customer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui|{
            let email_search = TextEdit::singleline(&mut self.customer.email)
            .hint_text("Customer Email")
            .ui(ui);

            if email_search.lost_focus() && !self.customer.email.is_empty() {
                let _ = self.action_tx.try_send(UiAction::SearchCustomerEmail(self.customer.email.clone()));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let phone_search = TextEdit::singleline(&mut self.address.phone)
                .hint_text("Customer Phone #")
                .ui(ui);

                if phone_search.lost_focus() && !self.address.phone.is_empty() {
                    let _ = self.action_tx.try_send(UiAction::SearchCustomerPhone(self.address.phone.clone()));
                }
            });
        });

        ui.vertical_centered(|ui| {

        });
    }

    pub fn create_order(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui|{
            ui.vertical_centered(|ui| {
                let res = TextEdit::singleline(&mut self.odoo_product_search)
                .hint_text("Search for a product")
                .ui(ui);

                if res.has_focus() && res.changed() && self.odoo_product_search.len() > 1  {
                    if self.last_search_time.elapsed() > Duration::from_millis(200) {
                        self.last_search_time = Instant::now();
                        let _ = self.action_tx.try_send(UiAction::SearchProduct(self.odoo_product_search.clone()));
                    }
                }
                

                // if ui.button("Search").clicked() && !self.odoo_product_search.is_empty() {
                //     let _ = self.action_tx.try_send(UiAction::SearchProduct(self.odoo_product_search.clone()));
                // }
            });
        });

        ui.add_space(10.);

        ui.columns(2, |ui| {
            ui[0].vertical_centered(|ui| {
                ui.group(|ui| {
                    Grid::new("Product Selection Grid")
                    .num_columns(6)
                    .min_col_width(ui.available_width()/6.1)
                    .max_col_width(ui.available_width()/6.1)
                    .striped(true)
                    .with_row_color(return_colors)
                    .show(ui, |ui| {
                        ui.colored_label(ui.style().visuals.error_fg_color, "Product Name");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Product Code");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Qty Avail");
                        ui.colored_label(ui.style().visuals.error_fg_color, "List Price");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Standard Price");
                        ui.label("");
                        ui.end_row();

                        for product in self.searched_products.iter() {
                            ui.label(&product.name);
                            ui.label(&product.default_code);
                            ui.label(product.qty_available.to_string());
                            ui.label(format!(" $ {:.2}", product.list_price));
                            ui.label(format!(" $ {:.2}", product.standard_price));
                            if ui.button("Add +").clicked() {
                                let _ = self.action_tx.try_send(UiAction::AddProduct(product.clone()));
                            }
                            ui.end_row();
                        }
                        
                        ui.end_row();
                    });
                });

                ui.add_space(30.);

                ui.spacing_mut().combo_height = 300.;

                ui.group(|ui| {
                    ui.heading("Device Details");
                    ui.separator();
                    ui.add_space(10.);
                

                    let selected_text = self.service_details.device.as_str().to_string();
                    let device_selected = &mut self.service_details.device;

                    ui.add_space(5.);

                    ComboBox::new("Device Selection", "")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for device in Device::VALUES {
                            ui.selectable_value(
                                device_selected,
                                device.clone(),
                                device.as_str(),
                            );
                        }
                    });

                    let selected_text = self.service_details.device_mfg.as_str().to_string();
                    let selected = &mut self.service_details.device_mfg;
                    let current_selection = selected.clone();

                    ui.add_space(5.);

                    ui.scope_builder(
                    UiBuilder::new()
                    .layout(Layout::from_main_dir_and_cross_align(Direction::LeftToRight, Align::Min)), 
                    |ui| {
                        let is_sizing_pass = ui.is_sizing_pass();
                        let available_width = ui.available_width();
                        let combo_width = 115.0; // Fixed width of each ComboBox
                        let total_content_width = combo_width * 2.0; // Two ComboBoxes side by side

                        // In the rendering pass, add spacing to center the content
                        if !is_sizing_pass && available_width > total_content_width {
                            let padding = (available_width - total_content_width) / 2.0;
                            ui.add_space(padding);
                        }

                        ComboBox::new("Device Mfg Selection", "")
                        .selected_text(selected_text)
                        .width(115.)
                        .show_ui(ui, |ui| {
                            for device in DeviceMfg::VALUES {
                                ui.selectable_value(
                                    selected,
                                    device.clone(),
                                    device.as_str(),
                                );
                            }
                        });

                        // Use egui memory to track if we've already done the sizing pass
                        let sizing_pass_done = ui.memory(|mem| mem.data.get_temp::<bool>(Id::new("combo_sizing_done")).unwrap_or(false));

                        if is_sizing_pass && !sizing_pass_done {
                            ui.ctx().request_discard("Centering ComboBox sizing pass");
                            ui.ctx().request_repaint();
                            ui.memory_mut(|mem| mem.data.insert_temp(Id::new("combo_sizing_done"), true));
                        }

                        // Reset the flag if the UI is repainted for other reasons (e.g., window resize)
                        if !is_sizing_pass && sizing_pass_done {
                            ui.memory_mut(|mem| mem.data.insert_temp(Id::new("combo_sizing_done"), false));
                        }
                    });

                    if *selected == DeviceMfg::PcLaptops {
                        let selected_text = self.service_details.device_desktop.as_str().to_string();
                        let selected = &mut self.service_details.device_desktop;
                        let current_selection = selected.clone();

                        ui.add_space(5.);

                        if *device_selected == Device::Desktop {
                            ComboBox::new("Desktop Selection", "")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for device in DesktopModel::VALUES {
                                    ui.selectable_value(
                                        selected,
                                        device.clone(),
                                        device.as_str(),
                                    );
                                }
                            });

                            if current_selection != *selected {
                                // let _ = self.data_selection_tx.try_send(selected.clone());
                            }
                        } else if *device_selected == Device::Laptop {
                            let selected_text = self.service_details.device_laptop.as_str().to_string();
                            let selected = &mut self.service_details.device_laptop;
                            let current_selection = selected.clone();

                            ui.add_space(5.);

                            ComboBox::new("Laptop Selection", "")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for device in LaptopModel::VALUES {
                                    ui.selectable_value(
                                        selected,
                                        device.clone(),
                                        device.as_str(),
                                    );
                                }
                            });

                            if current_selection != *selected {
                                // let _ = self.data_selection_tx.try_send(selected.clone());
                            }
                        }
                    }
                
                    TextEdit::singleline(&mut self.service_details.device_pw).hint_text("Device Password").ui(ui);
                    TextEdit::singleline(&mut self.service_details.device_serial).hint_text("Device Serial").ui(ui);
                    let cord = &mut self.service_details.power_cord;
                    // ui.horizontal(|ui| {
                        ui.checkbox(cord, "Power Cord?");
                    // });

                    if *cord {
                        TextEdit::singleline(&mut self.service_details.power_cord_serial).hint_text("Power Cord Serial").ui(ui);
                    }

                    TextEdit::multiline(&mut self.service_details.checkin_notes).hint_text("Check-In Notes").desired_rows(6).ui(ui);
                });
            });
            
            ui[1].vertical_centered(|ui| {
                ui.group(|ui| {

                    ui.heading(RichText::new("Added Products").underline().font(FontId::proportional(15.)));
                    ui.add_space(10.);
                    ui.separator();
                    ui.add_space(10.);

                    Grid::new("Added Products Grid")
                    .num_columns(6)
                    .min_col_width(ui.available_width()/6.1)
                    .max_col_width(ui.available_width()/6.1)
                    .striped(true)
                    .with_row_color(return_colors)
                    .show(ui, |ui| {
                        ui.colored_label(ui.style().visuals.error_fg_color, "Product Name");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Product Code");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Qty Avail");
                        ui.colored_label(ui.style().visuals.error_fg_color, "List Price");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Standard Price");
                        ui.label("");
                        ui.end_row();

                        for product in self.added_products.iter() {
                            ui.label(&product.name);
                            ui.label(&product.default_code);
                            ui.label(product.qty_available.to_string());
                            ui.label(format!(" $ {:.2}", product.list_price));
                            ui.label(format!(" $ {:.2}", product.standard_price));
                            if ui.button("Remove -").clicked() {
                                let _ = self.action_tx.try_send(UiAction::RemoveProduct(product.clone()));
                            }
                            ui.end_row();
                        }
                        
                        ui.end_row();
                    });
                });
            });
        });
    }
}
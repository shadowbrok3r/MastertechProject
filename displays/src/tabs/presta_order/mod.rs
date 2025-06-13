use web_time::{Duration, Instant};
use crossbeam::channel::{Receiver, Sender};
use database::{schema::{odoo::{search_odoo_products, ExtraInventoryData}, prestashop::{Address, Customer, Order, ServiceOrder}, CustomerData}, PlatformSpawner, Spawner};
use eframe::egui::{Align, CentralPanel, Grid, Layout, ScrollArea, TextEdit, TopBottomPanel, Ui, Widget};

use crate::modals::tabs::return_colors;



pub struct PrestashopOrderForm {
    order: Order,
    customer: Customer,
    address: Address,
    service_order: ServiceOrder,

    state: UiState,
    data: UiData,

    odoo_product_search: String,
    products: Vec<ExtraInventoryData>,

    last_search_time: Instant,

    action_tx: Sender<UiAction>,
    action_rx: Receiver<UiAction>,
    search_results_tx: Sender<UiData>,
    search_results_rx: Receiver<UiData>,
    odoo_search_tx: Sender<Vec<ExtraInventoryData>>,
    odoo_search_rx: Receiver<Vec<ExtraInventoryData>>,
}

#[derive(Default)]
pub struct UiData {
    customers: Vec<Customer>,
    addresses: Vec<Address>,
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
            odoo_product_search: String::new(),
            products: vec![],
            state: UiState::SelectCustomer,
            data: UiData::default(),
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
                UiAction::SearchCustomerEmail(email) => {
                    let search_email = email.clone();
                    let tx = self.search_results_tx.clone();
                    if !search_email.is_empty() {
                        PlatformSpawner::spawn(async move {
                            match Customer::find_customer_by_email(&search_email).await {
                                Ok((customer, address)) => {
                                    let _ = tx.try_send(UiData { customers: vec![customer], addresses: vec![address] });
                                },
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
                                Ok((customer, address)) => {
                                    let _ = tx.try_send(UiData { customers: vec![customer], addresses: vec![address] });
                                },
                                Err(e) => log::error!("Error Pulling customer info via email: {e:?}"),
                            }
                        });
                    }
                },
                UiAction::NewState(state) => {
                    self.state = state;
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
                UiAction::AddProduct(_product) => {

                }
            }
        }
    
        if let Ok(products) = self.odoo_search_rx.try_recv() {
            ui.ctx().request_repaint();
            self.products = products.clone();
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
        .num_columns(5)
        .min_col_width(ui.available_width()/5.)
        .max_col_width(ui.available_width()/5.)
        .striped(true)
        .with_row_color(return_colors)
        .show(ui, |ui| {
            ui.label("First Name");
            ui.label("Last Name");
            ui.label("Email");
            ui.label("Address");
            ui.label("");
            ui.end_row();

            for (customer, address) in self.data.customers.iter().zip(self.data.addresses.iter()) {
                ui.label(&customer.firstname);
                ui.label(&customer.lastname);
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

        Grid::new("Product Selection Grid")
        .num_columns(6)
        .min_col_width(ui.available_width()/6.)
        .max_col_width(ui.available_width()/6.)
        .striped(true)
        .with_row_color(return_colors)
        .show(ui, |ui| {
            ui.label("Product Name");
            ui.label("Product Code");
            ui.label("Qty Avail");
            ui.label("List Price");
            ui.label("Standard Price");
            ui.label("");
            ui.end_row();

            for product in self.products.iter() {
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
    }
}
use database::{schema::{odoo::{search_odoo_products, ExtraInventoryData}, prestashop::{Address, Customer, DesktopModel, Device, DeviceMfg, LaptopModel, Order, PrestashopOrderType, ServiceOrder}, User}, PlatformSpawner, Spawner};
use eframe::egui::{pos2, vec2, Align, CentralPanel, Checkbox, ComboBox, Direction, FontId, Frame, Grid, Id, Layout, Rect, RichText, ScrollArea, TextEdit, TopBottomPanel, Ui, UiBuilder, Widget};
use crate::{get_current_user_from_auth, get_database_users, modals::tabs::return_colors};
use crossbeam::channel::{Receiver, Sender};
use web_time::{Duration, Instant};
use itertools::Itertools;

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

            action_tx, action_rx,
            search_results_tx, search_results_rx,
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

        eframe::egui::Panel::top("PrestashopOrderTopPanel")
        .exact_height(25.)
        .frame(Frame::dark_canvas(ui.style()))
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
                        ui.add_space(ui.available_width()/4.);
                        ui.label("Creating Order for ");
                        ui.add_space(5.);
                        if ui.button(
                            RichText::new(
                            format!("{} {}", self.customer.firstname, self.customer.lastname)
                            )
                            .color(ui.style().visuals.error_fg_color)
                        ).clicked() {
                            let _ = self.action_tx.try_send(UiAction::NewState(UiState::SelectCustomer));
                        }
                    },
                    UiState::SelectCustomer => {
                        ui.add_space(ui.available_width()/4.);
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
        ui.add_space(20.);

        ui.group(|ui| {
            Grid::new("Customer Selection Grid")
            .num_columns(6)
            .spacing([5.0, 8.])
            .min_col_width(ui.available_width()/6.1)
            .max_col_width(ui.available_width()/6.1)
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
                        let _ = self.action_tx.try_send(UiAction::SetSelectedCustomer((customer.clone(), address.clone())));
                    }
                    ui.end_row();
                }
                
                ui.end_row();
            });
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

        ui.vertical_centered(|_ui| {

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

        ui.add_space(10.);

        #[allow(const_item_mutation)]
        let product_search_rect = &mut Rect::ZERO;

        ui.columns(2, |ui| {
            ui[0].vertical_centered(|ui| {
                ui.spacing_mut().combo_height = 300.;

                ui.horizontal_top(|ui| {
                    ui.group(|ui| {    
                        Grid::new("Selected Customer Grid")
                        .num_columns(2)
                        .spacing([5.0, 8.])
                        .min_col_width(180.)
                        .max_col_width(180.)
                        .striped(true)
                        .with_row_color(return_colors)
                        .show(ui, |ui| {
                            let customer = &self.customer;
                            let address = &self.address;
                            ui.colored_label(ui.style().visuals.error_fg_color, "ID");
                            ui.label(&customer.id);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Name");
                            ui.label(format!("{} {}", customer.firstname, customer.lastname));
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Phone #");
                            ui.label(&address.phone);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Mobile #");
                            ui.label(&address.phone_mobile);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Email");
                            ui.label(&customer.email);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "City");
                            ui.label(&address.city);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Address");
                            ui.label(&address.address1);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Address 2");
                            ui.label(&address.address2);
                            ui.end_row();
                            ui.colored_label(ui.style().visuals.error_fg_color, "Zipcode");
                            ui.label(&address.postcode);
                            ui.end_row();
                        });
                    });

                    ui.add_space(50.);

                    ui.vertical_centered(|ui| {
                        ui.set_width(350.);
                        ui.group(|ui| {
                            ui.heading(RichText::new("Device Details").underline().font(FontId::proportional(15.)));
                            ui.add_space(10.);
                            ui.separator();
                            ui.add_space(10.);
                        
                            ui.scope_builder(
                            UiBuilder::new()
                            .layout(Layout::from_main_dir_and_cross_align(Direction::LeftToRight, Align::Min)), 
                            |ui| {
                                let is_sizing_pass = ui.is_sizing_pass();
                                let available_width = ui.available_width();
                                let combo_width = 115.0; // Fixed width of each ComboBox
                                let total_content_width = combo_width * 3.0; // Two ComboBoxes side by side

                                // In the rendering pass, add spacing to center the content
                                if !is_sizing_pass && available_width > total_content_width {
                                    let padding = (available_width - total_content_width) / 2.0;
                                    ui.add_space(padding);
                                }

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

                                ui.add_space(5.);

                                let selected_text = self.service_details.device_mfg.as_str().to_string();
                                let selected = &mut self.service_details.device_mfg;
                                let _current_selection = selected.clone();

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

                            ui.add_space(5.);
                            TextEdit::singleline(&mut self.service_details.device_pw).hint_text("Device Password").ui(ui);
                            ui.add_space(5.);
                            let rect = TextEdit::singleline(&mut self.service_details.device_serial).hint_text("Device Serial").ui(ui).rect;
                            ui.add_space(5.);
                            ui.horizontal_top(|ui| {
                                let cord = &mut self.service_details.power_cord;
                                let checkbox_pos = pos2(rect.min.x, rect.max.y + 5.0); // 5.0 for a small gap
                                let new_rect = Rect::from_min_size(checkbox_pos, vec2(100., 20.));
                                ui.put(new_rect, Checkbox::new(cord, "Power Cord?"));
                                // ui.add_space(ui.available_width()/3.5);
                                ui.add_space(10.);
                                if *cord {
                                    TextEdit::singleline(&mut self.service_details.power_cord_serial)
                                        .hint_text("Power Cord Serial")
                                        .desired_width(150.)
                                        .ui(ui);
                                }
                            });
                            ui.add_space(5.);
                            TextEdit::multiline(&mut self.service_details.checkin_notes).hint_text("Check-In Notes").desired_rows(6).ui(ui);

                            ui.add_space(13.);
                        });
                    });

                    ui.add_space(10.);
                });

                ui.add_space(30.);

                ui.horizontal_top(|ui| {
                    ui.add_space(ui.available_width()/2.8);
                    let res = TextEdit::singleline(&mut self.odoo_product_search)
                    .desired_width(150.)
                    .hint_text("Search for a product")
                    .ui(ui);

                    if res.has_focus() && res.changed() && self.odoo_product_search.len() > 1  {
                        if self.last_search_time.elapsed() > Duration::from_millis(50) {
                            self.last_search_time = Instant::now();
                            let _ = self.action_tx.try_send(UiAction::SearchProduct(self.odoo_product_search.clone()));
                        }
                    }
                    ui.add_space(5.);
                    if ui.button("Clear").clicked() {
                        self.odoo_product_search.clear();
                    }                                
                });

                ui.add_space(10.);

                *product_search_rect = ui.group(|ui| {
                    ui.heading(RichText::new("Product Search Results").underline().font(FontId::proportional(15.)));
                    ui.add_space(10.);
                    ui.separator();
                    ui.add_space(10.);
                    
                    Grid::new("Product Selection Grid")
                    .num_columns(5)
                    .spacing([5., 8.])
                    .min_col_width(ui.available_width()/5.1)
                    .max_col_width(ui.available_width()/5.1)
                    .striped(true)
                    .with_row_color(return_colors)
                    .show(ui, |ui| {
                        // ui.colored_label(ui.style().visuals.error_fg_color, "Product Name");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Product Code");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Qty Avail");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Cost");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Standard Price");
                        ui.label("");
                        ui.end_row();

                        for product in self
                            .searched_products
                            .iter()
                            .filter(|p| 
                                !p.default_code
                                .to_lowercase()
                                .ends_with("/xidax")
                            ) 
                        {
                            // ui.label(&product.name);
                            ui.label(&product.default_code);
                            ui.label(product.qty_available.to_string());
                            ui.label(format!(" $ {:.2}", product.list_price));
                            ui.label(format!(" $ {:.2}", product.standard_price));
                            if ui.button("Add +").clicked() {
                                let _ = self.action_tx.try_send(UiAction::AddProduct(product.clone()));
                            }
                            ui.end_row();
                        }
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    });
                }).response.rect;
            });
            
            ui[1].vertical_centered(|ui| {
                ui.vertical_centered(|ui| {
                    ui.set_width(435.);
                    ui.group(|ui| {
                        ui.heading(RichText::new("Service Details").underline().font(FontId::proportional(15.)));
                        ui.add_space(10.);
                        ui.separator();
                        ui.add_space(10.);

                        let current_name = self.store_users
                            .iter()
                            .filter(|u| u.is_active())
                            .find(|u| u.get_id() == self.sales_rep.get_id())
                            .map(|u| u.get_username().to_owned())
                            .unwrap_or_else(|| "Sales Rep".to_string());

                        let current_split_rep = self.store_users
                            .iter()
                            .filter(|u| u.is_active() && u.get_id() != self.sales_rep.get_id())
                            .find_or_first(|u| u.get_id() == self.split_rep.get_id())
                            .map(|u| u.get_username().to_owned())
                            .unwrap_or_else(|| "Split Rep".to_string());

                        let my_store = self.sales_rep.get_store();
                        let mut sorted_users: Vec<&User> = self.store_users
                            .iter()
                            .filter(|u| u.is_active())
                            .collect();

                        sorted_users.sort_by_key(|u| {
                            (
                                // same‑store? (false=first, true=later)
                                u.get_store() != my_store,
                                // then by username (case‑insensitive)
                                u.get_username().to_lowercase(),
                            )
                        });

                        ui.horizontal(|ui| {
                            ui.colored_label(ui.style().visuals.error_fg_color, "Sales Rep:");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ComboBox::from_id_salt("Sales Rep Selection")
                                .selected_text(&current_name)
                                .width(125.)
                                .show_ui(ui, |ui| {
                                    for user in sorted_users.iter().cloned() {
                                        ui.selectable_value(
                                            &mut self.sales_rep,       // current_value: &mut RecordId
                                            user.clone(),    // selected_value: RecordId
                                            user.get_username(),      // text: &str or String
                                        );
                                    }
                                });
                            });
                        });

                        ui.add_space(5.);

                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.add_split_rep, RichText::new("Split Rep:").color(ui.style().visuals.error_fg_color));
                            
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.add_split_rep {
                                    ComboBox::from_id_salt("Split Rep Selection")
                                    .selected_text(&current_split_rep)
                                    .width(125.)
                                    .show_ui(ui, |ui| {
                                        // let users = sorted_users.clone
                                        for user in sorted_users.iter().filter(|u| u.get_id() != self.sales_rep.get_id()).cloned() {
                                            ui.selectable_value(
                                                &mut self.split_rep,       // current_value: &mut RecordId
                                                user.clone(),    // selected_value: RecordId
                                                user.get_username(),      // text: &str or String
                                            );
                                        }
                                    });
                                }
                            });
                        });

                        ui.add_space(5.);

                        ui.horizontal(|ui| {
                            ui.colored_label(ui.style().visuals.error_fg_color, "Order Type:");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ComboBox::new("Order State", "")
                                .selected_text(self.order_state.as_str())
                                .width(125.)
                                .show_ui(ui, |ui| {
                                    for status in PrestashopOrderType::VALUES.iter() {
                                        ui.selectable_value(
                                            &mut self.order_state, 
                                            status.clone(), 
                                            status.as_str()
                                        );
                                    }
                                });
                            });
                        });
                            
                        ui.add_space(5.);
                        ui.add_space(30.);
                        ui.add_space(13.);
                    });
                });

                // Calculate the space needed to align the "Added Products" group with the "Product Search Results" group
                let target_y = product_search_rect.min.y; // Top y-coordinate of "Product Search Results"
                let current_y = ui.cursor().min.y; // Current y-position in the right column
                let space_to_add = target_y - current_y; // Space needed to align tops

                if space_to_add > 0.0 {
                    ui.add_space(space_to_add); // Add space to push "Added Products" down
                }

                ui.group(|ui| {
                    ui.heading(RichText::new("Added Products").underline().font(FontId::proportional(15.)));
                    ui.add_space(10.);
                    ui.separator();
                    ui.add_space(10.);

                    Grid::new("Added Products Grid")
                    .num_columns(5)
                    .spacing([5.0, 8.])
                    .min_col_width(ui.available_width()/5.1)
                    .max_col_width(ui.available_width()/5.1)
                    .striped(true)
                    .with_row_color(return_colors)
                    .show(ui, |ui| {
                        // ui.colored_label(ui.style().visuals.error_fg_color, "Product Name");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Product Code");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Qty Avail");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Cost");
                        ui.colored_label(ui.style().visuals.error_fg_color, "Standard Price");
                        ui.label("");
                        ui.end_row();

                        let total = &mut 0.0;
                        for product in self.added_products.iter() {
                            *total += product.list_price;
                            // ui.label(&product.name);
                            ui.label(&product.default_code);
                            ui.label(product.qty_available.to_string());
                            ui.label(format!(" $ {:.2}", product.list_price));
                            ui.label(format!(" $ {:.2}", product.standard_price));
                            if ui.button("Remove -").clicked() {
                                let _ = self.action_tx.try_send(UiAction::RemoveProduct(product.clone()));
                            }
                            ui.end_row();
                        }

                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.colored_label(ui.style().visuals.error_fg_color, format!("Subtotal: $ {:.2}", total));
                        
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
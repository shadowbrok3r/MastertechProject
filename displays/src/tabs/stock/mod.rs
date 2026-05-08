use eframe::egui::{Align2, Area, Button, CentralPanel, Color32, ComboBox, Frame, Hyperlink, Id, Order, RichText, Spinner, TextEdit, Ui, Widget, scroll_area};
use crate::tabs::stock::store_inventory_viewer::{ExtraInventoryData, StockQuantityData, StockQuantityViewer};
use crate::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use crate::{PlatformSpawner, Spawner, TaskUiActions, get_current_user_from_auth};
use database::schema::{Store, UserAuthorization, prestashop::{Customer, Address, xml::{modify_xml, remove_xml_tag}}};
use database::xidax_order_url;
use egui_data_table::Renderer;
use log::info;

pub mod row_viewer;
pub mod stock_operations;
pub mod store_inventory_viewer;

pub use row_viewer::*;
pub use stock_operations::*;

// #[derive(Default)]
pub struct StockTable {
    stock_selection: StockSelection,
    inventory_serials_table: egui_data_table::DataTable<SerialsData>,
    inventory_serials_viewer: SerialsViewer,
    stock_quantity_viewer: StockQuantityViewer,
    stock_quantity_table: egui_data_table::DataTable<StockQuantityData>,
    // Cost breakdown
    cost_breakdown_viewer: CostBreakdownViewer,
    cost_breakdown_table: egui_data_table::DataTable<CostBreakdownData>,
    cost_order_id: String,
    cost_loading: bool,
    cost_summary: Option<CostBreakdownSummary>,
    // Systems In-Store
    systems_in_store_viewer: SystemInStoreViewer,
    systems_in_store_table: egui_data_table::DataTable<SystemInStoreData>,
    systems_order_id: String,
    systems_loading: bool,
    systems_first_load: bool,
    is_admin: bool,
    first_run: bool,
    pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    pub extra_stock_channel: (Sender<Vec<ExtraInventoryData>>, Receiver<Vec<ExtraInventoryData>>),
    pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    pub cost_channel: (Sender<Vec<CostBreakdownData>>, Receiver<Vec<CostBreakdownData>>),
    pub cost_summary_channel: (Sender<CostBreakdownSummary>, Receiver<CostBreakdownSummary>),
    pub systems_channel: (Sender<Vec<SystemInStoreData>>, Receiver<Vec<SystemInStoreData>>),
    pub systems_add_channel: (Sender<SystemInStoreData>, Receiver<SystemInStoreData>),
    pub systems_task_channel: (Sender<SystemInStoreData>, Receiver<SystemInStoreData>),
    store_selection: u64,
    // Customer change modal state
    pub customer_change_channel: (Sender<CustomerChangeRequest>, Receiver<CustomerChangeRequest>),
    pub customer_search_results_channel: (Sender<Vec<(Customer, Address)>>, Receiver<Vec<(Customer, Address)>>),
    customer_modal_open: bool,
    customer_modal_order_id: String,
    customer_modal_current_name: String,
    customer_search_query: String,
    customer_search_type: CustomerSearchType,
    customer_search_results: Vec<(Customer, Address)>,
    customer_searching: bool,
}

#[derive(Default, PartialEq, Clone)]
pub enum CustomerSearchType {
    #[default]
    Email,
    Phone,
}

#[derive(Default, PartialEq)]
pub enum StockSelection {
    #[default]
    CompanyStock,
    StoreInventory,
    CostBreakdown,
    SystemsInStore,
}

impl StockSelection {
    fn as_str(&self) -> &str {
        match self {
            StockSelection::CompanyStock => "Company Stock",
            StockSelection::StoreInventory => "Store Inventory",
            StockSelection::CostBreakdown => "Cost Breakdown",
            StockSelection::SystemsInStore => "Systems In-Store",
        }
    }
}

impl Default for StockTable {
    fn default() -> Self {
        let stock_channel = <Vec<RawStockData>>::create_unbounded_channel();
        let serial_channel = <SerialData>::create_unbounded_channel();
        let extra_stock_channel = <Vec<ExtraInventoryData>>::create_unbounded_channel();
        let cost_channel = <Vec<CostBreakdownData>>::create_unbounded_channel();
        let cost_summary_channel = <CostBreakdownSummary>::create_unbounded_channel();
        let systems_channel = <Vec<SystemInStoreData>>::create_unbounded_channel();
        let systems_add_channel = <SystemInStoreData>::create_unbounded_channel();
        let systems_task_channel = <SystemInStoreData>::create_unbounded_channel();
        let customer_change_channel: (Sender<CustomerChangeRequest>, Receiver<CustomerChangeRequest>) = crossbeam::channel::unbounded();
        let customer_search_results_channel: (Sender<Vec<(Customer, Address)>>, Receiver<Vec<(Customer, Address)>>) = crossbeam::channel::unbounded();

        let mut inventory_serials_viewer = SerialsViewer::default();
        inventory_serials_viewer.stock_tx = Some(serial_channel.0.clone());

        let systems_in_store_viewer = SystemInStoreViewer::new(
            systems_task_channel.0.clone(),
            customer_change_channel.0.clone(),
        );

        // Check if current user is admin
        let is_admin = get_current_user_from_auth()
            .map(|user| user.is_admin() | user.is_manager())
            .unwrap_or(false);

        Self { 
            stock_selection: Default::default(), 
            inventory_serials_table: egui_data_table::DataTable::<SerialsData>::default(),
            inventory_serials_viewer,
            stock_quantity_viewer: StockQuantityViewer::default(),
            stock_quantity_table: egui_data_table::DataTable::<StockQuantityData>::default(),
            cost_breakdown_viewer: CostBreakdownViewer::default(),
            cost_breakdown_table: egui_data_table::DataTable::<CostBreakdownData>::default(),
            cost_order_id: String::new(),
            cost_loading: false,
            cost_summary: None,
            systems_in_store_viewer,
            systems_in_store_table: egui_data_table::DataTable::<SystemInStoreData>::default(),
            systems_order_id: String::new(),
            systems_loading: false,
            systems_first_load: true,
            is_admin,
            first_run: true, 
            serial_channel, 
            extra_stock_channel, 
            stock_channel,
            cost_channel,
            cost_summary_channel,
            systems_channel,
            systems_add_channel,
            systems_task_channel,
            store_selection: Store::RIV.into_odoo_store_id() as u64,
            // Customer change modal
            customer_change_channel,
            customer_search_results_channel,
            customer_modal_open: false,
            customer_modal_order_id: String::new(),
            customer_modal_current_name: String::new(),
            customer_search_query: String::new(),
            customer_search_type: CustomerSearchType::default(),
            customer_search_results: Vec::new(),
            customer_searching: false,
        }
    }
}

impl StockTable {
    pub fn ui(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("StockTopPanel")
            .exact_size(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ComboBox::new("Stock Selection", "")
                    .selected_text(self.stock_selection.as_str())
                    .show_ui(ui, |ui| {
                        let selected = &mut self.stock_selection;
                        ui.selectable_value(
                            selected, 
                            StockSelection::CompanyStock,
                            StockSelection::CompanyStock.as_str()
                        );
                        ui.selectable_value(
                            selected, 
                            StockSelection::StoreInventory,
                            StockSelection::StoreInventory.as_str()
                        );
                        // Only show Cost Breakdown option for admins
                        if self.is_admin {
                            ui.selectable_value(
                                selected, 
                                StockSelection::CostBreakdown,
                                StockSelection::CostBreakdown.as_str()
                            );
                            ui.selectable_value(
                                selected,
                                StockSelection::SystemsInStore,
                                StockSelection::SystemsInStore.as_str()
                            );
                        }
                    });

                    ui.add_space(10.);

                    match self.stock_selection {
                        StockSelection::CompanyStock => {
                            TextEdit::singleline(&mut self.stock_quantity_viewer.filter)
                                .hint_text("Search for Item Code")
                                .ui(ui);

                            ui.add_space(10.);

                            if Button::new("Refresh").ui(ui).clicked() {
                                let stock_tx = self.extra_stock_channel.0.clone();
                                PlatformSpawner::spawn(async move {
                                    let stock = get_extra_stock_info(stock_tx.clone()).await;
                                    info!("Stock call: {stock:?}");
                                });
                            }
                            ui.add_space(10.);
                        },
                        StockSelection::StoreInventory => {
                            TextEdit::singleline(&mut self.inventory_serials_viewer.filter)
                                .hint_text("Search for Item Code or S/N")
                                .ui(ui);
        
                            ui.add_space(10.);
        
                            let selected = &mut self.store_selection;
                            let current = selected.clone();
                            let selected_text = Store::from_odoo_store_id(&selected.to_string()).as_str().to_string();
        
                            ComboBox::new("Store_Selection", "")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected, Store::RIV.into_odoo_store_id() as u64, Store::RIV.as_str());
                                    ui.selectable_value(selected, Store::LTN.into_odoo_store_id() as u64, Store::LTN.as_str());
                                    ui.selectable_value(selected, Store::MUR.into_odoo_store_id() as u64, Store::MUR.as_str());
                                    ui.selectable_value(selected, Store::ORE.into_odoo_store_id() as u64, Store::ORE.as_str());
                                    ui.selectable_value(selected, Store::SAN.into_odoo_store_id() as u64, Store::SAN.as_str());
                                });
        
                            if *selected != current {
                                let stock_tx = self.stock_channel.0.clone();
                                let store_selection = self.store_selection;
                                PlatformSpawner::spawn(async move {
                                    info!("Store: {:?}", store_selection);
                                    let stock = get_stock(stock_tx.clone(), store_selection).await;
                                    info!("Stock call: {stock:?}");
                                });
                            }
                            ui.add_space(10.);
        
                            if Button::new("Refresh").ui(ui).clicked() {
                                let stock_tx = self.stock_channel.0.clone();
                                let store_selection = self.store_selection;
                                PlatformSpawner::spawn(async move {
                                    let stock = get_stock(stock_tx.clone(), store_selection).await;
                                    info!("Stock call: {stock:?}");
                                });
                            }
                            ui.add_space(10.);
        
                            if Button::new("Refresh S/N Info").ui(ui).clicked() {
                                let tx = self.serial_channel.0.clone();
                                let data_table = self.inventory_serials_table.iter();
                                let sns = data_table.map(|r| r.1.clone()).collect::<Vec<String>>();
                                PlatformSpawner::spawn(async move {
                                    let _res = find_attached_serials(sns, tx.clone()).await;
                                    if let Err(e) = _res {
                                        log::error!("S/N Info call error: {e:?}");
                                    } else {
                                        log::info!("S/N Info call ran ok");
                                    }
                                });
                            }
                        },
                        StockSelection::CostBreakdown => {
                            TextEdit::singleline(&mut self.cost_order_id)
                                .desired_width(200.)
                                .hint_text("Enter Order ID")
                                .ui(ui);

                            ui.add_space(10.);

                            let can_search = !self.cost_order_id.is_empty() && !self.cost_loading;
                            if ui.add_enabled(can_search, Button::new("Search")).clicked() {
                                let cost_tx = self.cost_channel.0.clone();
                                let summary_tx = self.cost_summary_channel.0.clone();
                                let order_id = self.cost_order_id.clone();
                                self.cost_loading = true;
                                self.cost_summary = None;
                                self.cost_breakdown_viewer.clear_selection();
                                PlatformSpawner::spawn(async move {
                                    let _ = get_cost_breakdown(order_id, cost_tx, summary_tx).await;
                                });
                            }

                            if self.cost_loading {
                                ui.add_space(5.);
                                Spinner::new().size(18.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);

                            TextEdit::singleline(&mut self.cost_breakdown_viewer.filter)
                                .desired_width(250.)
                                .hint_text("Filter results")
                                .ui(ui);
                        },
                        StockSelection::SystemsInStore => {
                            // Store selection for Systems In-Store
                            let selected = &mut self.store_selection;
                            let current = *selected;
                            
                            let selected_text = Store::from_presta_store_id(&selected.to_string());
                            
                            ComboBox::new("Systems_Store_Selection", "")
                                .selected_text(selected_text.as_str())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected, Store::RIV.into_store_id() as u64, Store::RIV.as_str());
                                    ui.selectable_value(selected, Store::LTN.into_store_id() as u64, Store::LTN.as_str());
                                    ui.selectable_value(selected, Store::MUR.into_store_id() as u64, Store::MUR.as_str());
                                    ui.selectable_value(selected, Store::ORE.into_store_id() as u64, Store::ORE.as_str());
                                    ui.selectable_value(selected, Store::SAN.into_store_id() as u64, Store::SAN.as_str());
                                });
                            
                            // Trigger refresh when store changes
                            if *selected != current {
                                self.systems_first_load = true;
                            }
                            
                            ui.add_space(10.);

                            if Button::new("Refresh").ui(ui).clicked() || self.systems_first_load {
                                self.systems_first_load = false;
                                self.systems_loading = true;
                                let systems_tx = self.systems_channel.0.clone();
                                // store_selection is shared with the Store-Inventory view, which
                                // writes Odoo ids; normalize to a PrestaShop id so first-load and
                                // subsequent refreshes always hit the right backend identifier.
                                let store_id = Store::from_any_store_id(&self.store_selection.to_string())
                                    .into_store_id() as u64;
                                PlatformSpawner::spawn(async move {
                                    let _ = get_systems_in_store(store_id, systems_tx).await;
                                });
                            }
                            
                            ui.add_space(10.);
                            ui.separator();
                            ui.add_space(10.);
                            
                            // Manual order add
                            TextEdit::singleline(&mut self.systems_order_id)
                                .desired_width(120.)
                                .hint_text("Add Order ID")
                                .ui(ui);

                            ui.add_space(5.);

                            let can_add = !self.systems_order_id.is_empty() && !self.systems_loading;
                            if ui.add_enabled(can_add, Button::new("Add")).clicked() {
                                let systems_tx = self.systems_add_channel.0.clone();
                                let order_id = self.systems_order_id.clone();
                                self.systems_order_id.clear();
                                PlatformSpawner::spawn(async move {
                                    let _ = add_order_to_systems(order_id, systems_tx).await;
                                });
                            }

                            if self.systems_loading {
                                ui.add_space(5.);
                                Spinner::new().size(18.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                            }

                            ui.add_space(10.);

                            TextEdit::singleline(&mut self.systems_in_store_viewer.filter)
                                .desired_width(200.)
                                .hint_text("Filter systems")
                                .ui(ui);
                        },
                    }
                });
            });

        // Bottom panel for Cost Breakdown summary
        if self.stock_selection == StockSelection::CostBreakdown {
            // Calculate selection sums
            let selected_products = &self.cost_breakdown_viewer.selected_products;
            let (selected_unit_price_sum, selected_cost_sum): (f64, f64) = if !selected_products.is_empty() {
                self.cost_breakdown_table
                    .iter()
                    .filter(|row| selected_products.contains(&format!("{}:{}", row.0, row.1)))
                    .fold((0.0, 0.0), |(price_acc, cost_acc), row| {
                        // row.4 = unit_price, row.5 = cost, row.3 = quantity
                        (price_acc + (row.4 * row.3), cost_acc + (row.5 * row.3))
                    })
            } else {
                (0.0, 0.0)
            };
            let selection_count = selected_products.len();
            
            if let Some(ref summary) = self.cost_summary {
                eframe::egui::Panel::bottom("CostBreakdownBottom")
                    .exact_size(30.)
                    .show_inside(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 25.0;
                            Hyperlink::from_label_and_url(
                                RichText::new(self.cost_order_id.clone())
                                    .underline()
                                    .strong()
                                    .color(ui.style().visuals.error_fg_color),
                                xidax_order_url(&self.cost_order_id),
                            )
                            .open_in_new_tab(true)
                            .ui(ui);

                            ui.colored_label(Color32::LIGHT_BLUE, format!("Customer: {}", summary.customer_name));
                            
                            ui.label(format!("Order Total: ${:.2}", summary.order_total));
                            
                            ui.colored_label(
                                Color32::from_rgb(200, 100, 100),
                                format!("Cost: ${:.2}", summary.total_cost)
                            );
                            
                            let profit_color = if summary.profit >= 0.0 {
                                Color32::LIGHT_GREEN
                            } else {
                                ui.style().visuals.error_fg_color
                            };
                            ui.colored_label(
                                profit_color,
                                format!("Gross Profit: ${:.2}", summary.profit)
                            );
                            
                            // Show selection sum if any items are selected
                            if selection_count > 0 {
                                ui.separator();
                                ui.colored_label(
                                    Color32::GOLD,
                                    format!("Selected ({}):", selection_count)
                                );
                                ui.label(format!("Price: ${:.2}", selected_unit_price_sum));
                                ui.colored_label(
                                    Color32::from_rgb(200, 100, 100),
                                    format!("Cost: ${:.2}", selected_cost_sum)
                                );
                                let sel_profit = selected_unit_price_sum - selected_cost_sum;
                                let sel_profit_color = if sel_profit >= 0.0 {
                                    Color32::LIGHT_GREEN
                                } else {
                                    ui.style().visuals.error_fg_color
                                };
                                ui.colored_label(sel_profit_color, format!("Profit: ${:.2}", sel_profit));
                            }
                        });
                    });
            }
        }

        CentralPanel::default().show_inside(ui, |ui| {
            match self.stock_selection {
                StockSelection::CompanyStock => {
                    if self.stock_quantity_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.label("Pulling Company Stock Information..");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else {   
                        Renderer::new(
                            &mut self.stock_quantity_table,
                            &mut self.stock_quantity_viewer,
                        )
                        .with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                },
                StockSelection::StoreInventory => {
                    if self.inventory_serials_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.label("Pulling Store Stock Information..");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else {
                        Renderer::new(
                            &mut self.inventory_serials_table, 
                            &mut self.inventory_serials_viewer
                        ).with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                },
                StockSelection::CostBreakdown => {
                    if self.cost_loading {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("Fetching order cost breakdown...");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else if self.cost_breakdown_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("Enter an Order ID above and click Search to view cost breakdown.");
                        });
                    } else {
                        Renderer::new(
                            &mut self.cost_breakdown_table, 
                            &mut self.cost_breakdown_viewer
                        ).with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                },
                StockSelection::SystemsInStore => {
                    if self.systems_loading {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("Fetching systems in-store...");
                            Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        });
                    } else if self.systems_in_store_table.len() < 1 {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.);
                            ui.label("No systems found. Select a store and click Refresh to load systems.");
                        });
                    } else {
                        Renderer::new(
                            &mut self.systems_in_store_table, 
                            &mut self.systems_in_store_viewer
                        ).with_style_modify(|s| {
                            s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                    }
                    
                    // Show customer change modal if open
                    self.show_customer_modal(ui);
                },
            }
        });
    }

    pub fn first_run(&mut self) {
        if self.first_run {
            self.first_run = false;
            let stock_tx = self.extra_stock_channel.0.clone();
            PlatformSpawner::spawn(async move {
                let stock = get_extra_stock_info(stock_tx.clone()).await;
                log::info!("Stock call: {stock:?}");
            });

            let stock_tx = self.stock_channel.0.clone();
            let store_selection = self.store_selection;
            PlatformSpawner::spawn(async move {
                let stock = get_stock(stock_tx.clone(), store_selection).await;
                log::info!("Stock call: {stock:?}");
            });
            self.is_admin = get_current_user_from_auth()
                .map(|user| if user.get_username().is_empty() {
                    log::info!("User is empty");
                    false
                } else {
                    user.is_admin() | user.is_manager()
                })
                .unwrap_or(false);
        }
    }

    pub fn receive(&mut self, ui_actions_tx: Sender<TaskUiActions>) {
        if let Ok(stock_data) = self.stock_channel.1.try_recv() {
            let data: Vec<SerialsData> = stock_data
                .iter()
                .map(|stock_data| {
                    SerialsData(
                        stock_data.product_id.clone().1.clone(),
                        stock_data.lot_id.clone().1.parse::<String>().unwrap(),
                        "S/N Info ⮫".to_string(),
                        Store::from_odoo_store_id(&stock_data.location_id.0.to_string()).as_str().to_string(),
                        false,
                    )
                })
                .collect();

            let tx = self.serial_channel.0.clone();

            let sns = data.iter().map(|r| r.1.clone()).collect::<Vec<String>>();

            PlatformSpawner::spawn(async move {
                let _res = find_attached_serials(sns, tx.clone()).await;
            });

            self.inventory_serials_table.replace(data);
        }

        if let Ok(serial_data) = self.serial_channel.1.try_recv() {
            log::debug!("Serial Data: {:?}", serial_data);
            let mut data_table = self.inventory_serials_table.take();
            for data in data_table.iter_mut() {
                for serial_info in serial_data.result.iter() {
                    if data.1 == serial_info.name {
                        match serial_info.clone().bs_prest_ref {
                            BoolOrString::Bool(_) => {
                                data.2 = "Not Attached".to_string();
                                data.4 = false;
                            }
                            BoolOrString::String(order_num) => {
                                if !order_num.is_empty() {
                                    data.2 = order_num;
                                    data.4 = true;
                                } else {
                                    data.2 = "Not Attached".to_string();
                                    data.4 = false;
                                }
                            }
                        };
                    }
                }
            }
            self.inventory_serials_table.replace(data_table);
        }

        if let Ok(stock_inf) = self.extra_stock_channel.1.try_recv() {
            log::debug!("Serial Data: {:?}", stock_inf);
            let data: Vec<StockQuantityData> = stock_inf
                .iter()
                .map(|stock_data| {
                    StockQuantityData(
                        stock_data.display_name.clone(),
                        stock_data.qty_available.clone(),
                        stock_data.virtual_available.clone(),
                        stock_data.standard_price.clone(),
                        stock_data.list_price.clone(),
                    )
                })
                .collect();
            self.stock_quantity_table.replace(data);
        }

        // Handle cost breakdown data
        if let Ok(cost_data) = self.cost_channel.1.try_recv() {
            log::info!("Received cost breakdown data: {} items", cost_data.len());
            self.cost_loading = false;
            self.cost_breakdown_table.replace(cost_data);
        }

        // Handle cost breakdown summary
        if let Ok(summary) = self.cost_summary_channel.1.try_recv() {
            log::info!("Received cost summary: customer={}, total=${:.2}, cost=${:.2}, profit=${:.2}", 
                      summary.customer_name, summary.order_total, summary.total_cost, summary.profit);
            self.cost_summary = Some(summary);
        }

        // Handle systems in-store data
        if let Ok(systems_data) = self.systems_channel.1.try_recv() {
            log::info!("Received systems in-store data: {} systems", systems_data.len());
            self.systems_loading = false;
            self.systems_in_store_table.replace(systems_data);
        }

        // Handle single system add
        if let Ok(system_data) = self.systems_add_channel.1.try_recv() {
            log::info!("Adding system to table: {}", system_data.order_id);
            let mut data = self.systems_in_store_table.take();
            // Only add if not already in the table
            if !data.iter().any(|s| s.order_id == system_data.order_id) {
                data.push(system_data);
            }
            self.systems_in_store_table.replace(data);
        }

        // Handle single system task creation
        if let Ok(system_data) = self.systems_task_channel.1.try_recv() {
            log::info!("Creating task for system: {}", system_data.order_id);
            // Send the system data to the create task modal via TaskUiActions
            let _ = ui_actions_tx.try_send(TaskUiActions::OpenCreateTaskModalFromSystem(system_data));
        }

        // Handle customer change request (open modal)
        if let Ok(request) = self.customer_change_channel.1.try_recv() {
            log::info!("Opening customer change modal for order: {}", request.order_id);
            self.customer_modal_open = true;
            self.customer_modal_order_id = request.order_id;
            self.customer_modal_current_name = request.customer_name;
            self.customer_search_query.clear();
            self.customer_search_results.clear();
        }

        // Handle customer search results
        if let Ok(results) = self.customer_search_results_channel.1.try_recv() {
            log::info!("Received customer search results: {} customers", results.len());
            self.customer_search_results = results;
            self.customer_searching = false;
        }
    }

    /// Show customer change modal
    pub fn show_customer_modal(&mut self, ui: &mut Ui) {
        if !self.customer_modal_open {
            return;
        }

        // Dim background
        let screen_rect = ui.ctx().screen_rect();
        ui.painter().rect_filled(
            screen_rect,
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 180),
        );

        Area::new(Id::new("customer_change_modal"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .order(Order::Foreground)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style())
                    .fill(Color32::from_rgb(30, 30, 35))
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        ui.set_min_width(400.0);
                        ui.set_min_height(300.0);
                        
                        ui.vertical(|ui| {
                            // Header
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new("Change Customer").color(Color32::WHITE));
                                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                    if ui.button("✕").clicked() {
                                        self.customer_modal_open = false;
                                    }
                                });
                            });
                            
                            ui.separator();
                            
                            ui.label(format!("Order: {}", self.customer_modal_order_id));
                            ui.label(format!("Current Customer: {}", self.customer_modal_current_name));
                            
                            ui.add_space(10.0);
                            
                            // Search type selector
                            ui.horizontal(|ui| {
                                ui.label("Search by:");
                                ui.selectable_value(&mut self.customer_search_type, CustomerSearchType::Email, "Email");
                                ui.selectable_value(&mut self.customer_search_type, CustomerSearchType::Phone, "Phone");
                            });
                            
                            ui.add_space(5.0);
                            
                            // Search input
                            ui.horizontal(|ui| {
                                let hint = match self.customer_search_type {
                                    CustomerSearchType::Email => "Enter email address...",
                                    CustomerSearchType::Phone => "Enter phone number...",
                                };
                                
                                let response = TextEdit::singleline(&mut self.customer_search_query)
                                    .hint_text(hint)
                                    .desired_width(250.0)
                                    .ui(ui);
                                
                                let can_search = !self.customer_search_query.is_empty() && !self.customer_searching;
                                if ui.add_enabled(can_search, Button::new("Search")).clicked() || 
                                   (response.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) && can_search) 
                                {
                                    self.customer_searching = true;
                                    let query = self.customer_search_query.clone();
                                    let search_type = self.customer_search_type.clone();
                                    let tx = self.customer_search_results_channel.0.clone();
                                    
                                    PlatformSpawner::spawn(async move {
                                        let results = match search_type {
                                            CustomerSearchType::Email => Customer::find_customer_by_email(&query).await,
                                            CustomerSearchType::Phone => Customer::find_customer_by_phone(&query).await,
                                        };
                                        
                                        match results {
                                            Ok(customers) => { let _ = tx.try_send(customers); },
                                            Err(e) => log::error!("Customer search error: {:?}", e),
                                        }
                                    });
                                }
                                
                                if self.customer_searching {
                                    Spinner::new().size(16.0).ui(ui);
                                }
                            });
                            
                            ui.add_space(10.0);
                            
                            // Search results
                            if !self.customer_search_results.is_empty() {
                                ui.label(RichText::new(format!("Found {} customers:", self.customer_search_results.len()))
                                    .color(Color32::LIGHT_BLUE));
                                
                                eframe::egui::ScrollArea::vertical()
                                    .max_height(200.0)
                                    .show(ui, |ui| {
                                        for (customer, address) in self.customer_search_results.iter() {
                                            let name = format!("{} {}", customer.firstname, customer.lastname);
                                            let addr_info = if !address.address1.is_empty() {
                                                format!("{}, {}", address.address1, address.city)
                                            } else {
                                                "No address".to_string()
                                            };
                                            
                                            ui.group(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.vertical(|ui| {
                                                        // Show customer ID and name
                                                        ui.label(RichText::new(format!("[{}] {}", customer.id, name)).strong());
                                                        ui.label(RichText::new(&customer.email).small().color(Color32::GRAY));
                                                        ui.label(RichText::new(&addr_info).small().color(Color32::GRAY));
                                                    });
                                                    
                                                    ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                                        if ui.button("Select").clicked() {
                                                            // Update the order with new customer
                                                            let order_id = self.customer_modal_order_id.clone();
                                                            let customer_id = customer.id.clone();
                                                            let address_id = address.id.clone();
                                                            let customer_name = name.clone();
                                                            
                                                            log::info!("Updating order {} with customer {} ({}), address {}", 
                                                                      order_id, customer_name, customer_id, address_id);
                                                            
                                                            // Update the table immediately
                                                            let mut data = self.systems_in_store_table.take();
                                                            for system in data.iter_mut() {
                                                                if system.order_id == order_id {
                                                                    system.customer_id = customer_id.clone();
                                                                    system.customer_name = customer_name.clone();
                                                                }
                                                            }
                                                            self.systems_in_store_table.replace(data);
                                                            
                                                            // Async update to Prestashop
                                                            PlatformSpawner::spawn(async move {
                                                                update_order_customer(&order_id, &customer_id, &address_id).await;
                                                            });
                                                            
                                                            self.customer_modal_open = false;
                                                        }
                                                    });
                                                });
                                            });
                                        }
                                    });
                            } else if !self.customer_searching && !self.customer_search_query.is_empty() {
                                ui.label(RichText::new("No customers found.").color(Color32::GRAY));
                            }
                            
                            ui.add_space(10.0);
                            
                            // Cancel button
                            ui.horizontal(|ui| {
                                if ui.button("Cancel").clicked() {
                                    self.customer_modal_open = false;
                                }
                            });
                        });
                    });
            });
    }
}

/// Update order customer and address in Prestashop
async fn update_order_customer(order_id: &str, customer_id: &str, address_id: &str) {
    use database::schema::prestashop::Prestashop;
    
    let api = Prestashop::default();
    
    // Get the current order XML
    match api.request_raw_resource_by_id("orders", order_id).await {
        Ok(xml) => {
            // Update id_customer
            match modify_xml(&xml, "id_customer", customer_id) {
                Ok(xml_with_customer) => {
                    // Update id_address_invoice
                    match modify_xml(&xml_with_customer, "id_address_invoice", address_id) {
                        Ok(xml_with_address) => {
                            // Remove tax_exempt tag (required for update)
                            match remove_xml_tag(&xml_with_address, "tax_exempt") {
                                Ok(final_xml) => {
                                    match api.modify_prestashop_order(&final_xml).await {
                                        Ok(_) => log::info!("Successfully updated order {} customer to {}", order_id, customer_id),
                                        Err(e) => log::error!("Failed to update order {}: {:?}", order_id, e),
                                    }
                                }
                                Err(e) => log::error!("Failed to remove tax_exempt from XML: {:?}", e),
                            }
                        }
                        Err(e) => log::error!("Failed to modify id_address_invoice in XML: {:?}", e),
                    }
                }
                Err(e) => log::error!("Failed to modify id_customer in XML: {:?}", e),
            }
        }
        Err(e) => log::error!("Failed to get order XML for {}: {:?}", order_id, e),
    }
}

use eframe::egui::{Button, CentralPanel, Color32, ComboBox, Hyperlink, RichText, Spinner, TextEdit, TopBottomPanel, Ui, Widget, scroll_area};
use crate::tabs::stock::store_inventory_viewer::{ExtraInventoryData, StockQuantityData, StockQuantityViewer};
use crate::channel_manager::ChannelManager;
use crate::tabs::task_audit::row_viewer::BASE_URL;
use crossbeam::channel::{Receiver, Sender};
use crate::{get_current_user_from_auth, PlatformSpawner, Spawner};
use database::schema::{Store, UserAuthorization};
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
    store_selection: u64
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

        let mut inventory_serials_viewer = SerialsViewer::default();
        inventory_serials_viewer.stock_tx = Some(serial_channel.0.clone());

        let mut systems_in_store_viewer = SystemInStoreViewer::default();
        systems_in_store_viewer.task_create_tx = Some(systems_task_channel.0.clone());

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
            store_selection: Store::RIV.into_store_id() as u64,
        }
    }
}

impl StockTable {
    pub fn ui(&mut self, ui: &mut Ui) {
        TopBottomPanel::top("StockTopPanel")
            .exact_height(30.)
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
        
                            let selected_text = match selected {
                                76 => Store::RIV.as_str(),
                                73 => Store::LTN.as_str(),
                                74 => Store::MUR.as_str(),
                                78 => Store::WJ.as_str(),
                                75 => Store::ORE.as_str(),
                                77 => Store::SAN.as_str(),
                                _ => Store::RIV.as_str(),
                            };
        
                            ComboBox::new("Store_Selection", "")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected, 76, "RIV");
                                    ui.selectable_value(selected, 73, "LTN");
                                    ui.selectable_value(selected, 74, "MUR");
                                    ui.selectable_value(selected, 78, "WJ");
                                    ui.selectable_value(selected, 75, "ORE");
                                    ui.selectable_value(selected, 77, "SAN");
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
                            
                            let selected_text = match *selected {
                                7 => Store::RIV.as_str(),
                                8 => Store::LTN.as_str(),
                                10 => Store::MUR.as_str(),
                                11 => Store::WJ.as_str(),
                                12 => Store::SAN.as_str(),
                                14 => Store::ORE.as_str(),
                                _ => Store::RIV.as_str(),
                            };
                            
                            ComboBox::new("Systems_Store_Selection", "")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected, 7, "RIV");
                                    ui.selectable_value(selected, 8, "LTN");
                                    ui.selectable_value(selected, 10, "MUR");
                                    ui.selectable_value(selected, 11, "WJ");
                                    ui.selectable_value(selected, 14, "ORE");
                                    ui.selectable_value(selected, 12, "SAN");
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
                                let store_id = self.store_selection;
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
                TopBottomPanel::bottom("CostBreakdownBottom")
                    .exact_height(30.)
                    .show_inside(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 25.0;
                            Hyperlink::from_label_and_url(
                                RichText::new(self.cost_order_id.clone())
                                    .underline()
                                    .strong()
                                    .color(ui.style().visuals.error_fg_color),
                                format!("{}{}", BASE_URL, self.cost_order_id),
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

    pub fn receive(&mut self) {
        if let Ok(stock_data) = self.stock_channel.1.try_recv() {
            let data: Vec<SerialsData> = stock_data
                .iter()
                .map(|stock_data| {
                    SerialsData(
                        stock_data.product_id.clone().1.clone(),
                        stock_data.lot_id.clone().1.parse::<String>().unwrap(),
                        "S/N Info ⮫".to_string(),
                        match stock_data.location_id.0 {
                            76 => Store::RIV.as_str(),
                            73 => Store::LTN.as_str(),
                            74 => Store::MUR.as_str(),
                            78 => Store::WJ.as_str(),
                            75 => Store::ORE.as_str(),
                            77 => Store::SAN.as_str(),
                            _ => Store::RIV.as_str(),
                        }
                        .to_string(),
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
    }
    
    /// Get pending task creation request (if any)
    pub fn get_pending_task_create(&mut self) -> Option<SystemInStoreData> {
        self.systems_task_channel.1.try_recv().ok()
    }
}

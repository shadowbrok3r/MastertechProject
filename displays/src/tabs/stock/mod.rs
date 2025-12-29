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
    is_admin: bool,
    first_run: bool,
    pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    pub extra_stock_channel: (Sender<Vec<ExtraInventoryData>>, Receiver<Vec<ExtraInventoryData>>),
    pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    pub cost_channel: (Sender<Vec<CostBreakdownData>>, Receiver<Vec<CostBreakdownData>>),
    pub cost_summary_channel: (Sender<CostBreakdownSummary>, Receiver<CostBreakdownSummary>),
    store_selection: u64
}

#[derive(Default, PartialEq)]
pub enum StockSelection {
    #[default]
    CompanyStock,
    StoreInventory,
    CostBreakdown,
}

impl StockSelection {
    fn as_str(&self) -> &str {
        match self {
            StockSelection::CompanyStock => "Company Stock",
            StockSelection::StoreInventory => "Store Inventory",
            StockSelection::CostBreakdown => "Cost Breakdown",
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

        let mut inventory_serials_viewer = SerialsViewer::default();
        inventory_serials_viewer.stock_tx = Some(serial_channel.0.clone());

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
            is_admin,
            first_run: true, 
            serial_channel, 
            extra_stock_channel, 
            stock_channel,
            cost_channel,
            cost_summary_channel,
            store_selection: 76,
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
                                72 => Store::AF.as_str(),
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
                                    ui.selectable_value(selected, 72, "AF");
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
                    .filter(|row| selected_products.contains(&row.0))
                    .fold((0.0, 0.0), |(price_acc, cost_acc), row| {
                        // row.3 = unit_price, row.4 = cost, row.2 = quantity
                        (price_acc + (row.3 * row.2), cost_acc + (row.4 * row.2))
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
                            72 => Store::AF.as_str(),
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
    }
}

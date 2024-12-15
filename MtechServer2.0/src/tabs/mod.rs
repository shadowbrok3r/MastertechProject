pub mod customer;
pub mod github_issue;
pub mod query_builder;
pub mod quote_fulfilled_tasks;
pub mod seb_lookup;
pub mod user_chat;
pub mod web_console;
// pub mod terminal;

use database::schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, Store};
use eframe::egui::{ComboBox, Response, Ui, WidgetText};
use displays::{tabs::{logger::logger_ui, stock::{get_extra_stock_info, get_stock}}, FilterTasks};
use egui_dock::{NodeIndex, SurfaceIndex, TabViewer};
use super::app_state::MtechServerContext;
use wasm_bindgen_futures::spawn_local;
use log::info;

impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "Lil menu" => self.simple_demo_menu(ui),
            "My Tools" => self.shared_ctx.toolbox(ui),
            "Store Tasks" => self.shared_ctx.store_tasks(ui),
            "My Tasks" => self.shared_ctx.my_tasks(ui),
            "Ai" => self.shared_ctx.ai_playground(ui),
            "Web Console" => self.web_console(ui),
            "Completed Tasks" => self.shared_ctx.completed_tasks(ui),
            "Bug Report" => self.github(ui),
            "Logs" => logger_ui().show(ui),
            "Query Builder" => self.query_builder(ui),
            "Json Viewer" => self.shared_ctx.json_viewer(ui),
            "Store Stock" => self.shared_ctx.stock_viewer(ui),
            "SEB Lookup" => self.seb_lookup(ui),
            "Task Audit" => self.shared_ctx.task_table_viewer(ui),
            "Company Stock" => self.shared_ctx.stock_quantities_viewer(ui),
            // "Customers" => self.customer_view(ui),
            // "Terminal" => self.terminal(ui),
            _ => {}
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface_index: SurfaceIndex,
        _node_index: NodeIndex,
    ) {
        match tab.as_str() {
            "My Tasks" => self.simple_demo_menu(ui),
            "Store Tasks" => self.store_selection_menu(ui),
            _ => {
                ui.label(tab.to_string());
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        self.open_tabs.remove(tab);
        true
    }

    fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn add_popup(&mut self, ui: &mut Ui, surface_index: SurfaceIndex, node_index: NodeIndex) {
        ui.set_width(100.0);
        let tabs = &[
            &"Bug Report".to_string(),
            &"Terminal".to_string(),
            &"My Tools".to_string(),
            &"Web Console".to_string(),
            &"Store Tasks".to_string(),
            &"My Tasks".to_string(),
            &"Ai".to_string(),
            &"Completed Tasks".to_string(),
            &"Customers".to_string(),
            &"Logs".to_string(),
            &"Json Viewer".to_string(),
            &"Query Builder".to_string(),
            &"Store Stock".to_string(),
            &"Task Audit".to_string(),
            &"SEB Lookup".to_string(),
        ];

        for tab in tabs {
            if ui
                .selectable_label(self.open_tabs.contains(*tab), *tab)
                .clicked()
            {
                if !self.open_tabs.contains(*tab) {
                    self.on_add(surface_index, node_index);
                }
            }
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &Response) {
        if response.clicked() {
            match tab.as_str() {
                "Store Stock" => {
                    if let Some(usr) = &self.shared_ctx.current_user {
                        let stock_tx = self.shared_ctx.stock_channel.0.clone();
                        let store_selection = match usr.clone().store {
                            Store::RIV => 76,
                            Store::LTN => 73,
                            Store::MUR => 74,
                            Store::AF => 72,
                            Store::WJ => 78,
                            Store::ORE => 75,
                            Store::SAN => 77,
                        };
                        spawn_local(async move {
                            let stock = get_stock(stock_tx.clone(), store_selection).await;
                            info!("Stock call: {stock:?} for Store: {:?}", store_selection);
                        });
                    }
                },
                "Company Stock" => {
                    let ex_stock_tx = self.shared_ctx.extra_stock_channel.0.clone();
                    spawn_local(async move {
                        let stock_quantities = get_extra_stock_info(ex_stock_tx).await;
                        info!("Extra Stock {stock_quantities:?}");
                    });
                },
                "Completed Tasks" => {
                    // First, make sure there are no completed tasks loaded.
                    // If there no completed tasks, then load them.
                    // Otherwise, make sure the tasks that are completed 
                    // are for the correct selected store. 
                    if self.shared_ctx.tasks.filter_by_completion(true).is_empty() {
                        let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
                        let store_sel = self.shared_ctx.store_selection;
                        let store_selection = std::convert::Into::<Store>::into(store_sel).as_str().to_string();
                        
                        spawn_local(async move {
                            let get_completed_tasks_for_store = get_completed_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_completed_tasks_for_store: {get_completed_tasks_for_store:?}");
                        });
                        self.shared_ctx.task_layouts
                            .iter_mut()
                            .filter(|(page, _)| *page == "CompletedTasks")
                            .for_each(|(_, layout)| {
                                layout.loading = true;
                        });
                    }
                    
                },
                "Store Tasks" => {
                    // First, make sure there are no store tasks loaded.
                    // If there no store tasks, then load them.
                    // Otherwise, make sure the tasks that are loaded 
                    // for the selected store are for the CORRECT selected store. 
                    if self.shared_ctx.tasks.filter_by_completion(false).is_empty() {
                        let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
                        let len_tx = self.shared_ctx.payload_len_channel.0.clone();
                        let store_sel = self.shared_ctx.store_selection;
                        let store_selection = std::convert::Into::<Store>::into(store_sel).as_str().to_string();
                        
                        spawn_local(async move {
                            let get_tasks_for_store = get_tasks_for_store(tasks_tx, store_selection, len_tx).await;
                            info!("get_tasks_for_store: {get_tasks_for_store:?}");
                        });
                    }
                    
                },
                _ => {}
            }
            
        }
    }

}


impl MtechServerContext {
    pub fn simple_demo_menu(&mut self, ui: &mut Ui) {
        if ui.button("Open...").clicked() {
            ui.close_menu();
        }
        ui.menu_button("SubMenu", |ui| {
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_menu();
                }
                let _ = ui.button("Item");
            });
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_menu();
                }
                let _ = ui.button("Item");
            });
            let _ = ui.button("Item");
            if ui.button("Open...").clicked() {
                ui.close_menu();
            }
        });
        ui.menu_button("SubMenu", |ui| {
            let _ = ui.button("Item1");
            let _ = ui.button("Item2");
            let _ = ui.button("Item3");
            let _ = ui.button("Item4");
            if ui.button("Open...").clicked() {
                ui.close_menu();
            }
        });
        let _ = ui.button("Very long text for this item");
    }

    pub fn store_selection_menu(&mut self, ui: &mut Ui) {
        let selected = &mut self.shared_ctx.store_selection;
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
            let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
            let store_users_tx = self.shared_ctx.store_users_tx.clone();
            let store_selection = match selected.clone() {
                76 => Store::RIV,
                73 => Store::LTN,
                74 => Store::MUR,
                78 => Store::WJ,
                75 => Store::ORE,
                72 => Store::AF,
                77 => Store::SAN,
                _ => Store::RIV,
            };

            self.shared_ctx.store_users.clear();
            self.shared_ctx.tasks.clear();
            let len_tx = self.shared_ctx.payload_len_channel.0.clone();
            info!("Store: {store_selection:?}//{:?}", store_selection.clone().as_str().to_string());
            spawn_local(async move {
                let store_tasks = get_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string(), len_tx).await;
                let get_store_users = get_store_users(store_users_tx, store_selection).await;

                info!("get_tasks_for_store: {store_tasks:?}");
                info!("get_store_users: {get_store_users:?}");
            });
        }
    }

}
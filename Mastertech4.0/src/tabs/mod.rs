use database::schema::{utilities::{get_completed_tasks_for_store, get_tasks_for_store}, Store};
use displays::{tabs::{logger::logger_ui, stock::{get_extra_stock_info, get_stock}}, FilterTasks};
use egui_dock::{NodeIndex, SurfaceIndex, TabViewer};
use crate::app_state::MastertechContext;
use eframe::egui::{Ui, WidgetText};
use github::get_github_releases;
use std::sync::atomic::Ordering;
use log::{error, info};
use anyhow::Error;
use tokio::spawn;

pub mod file_browser;
pub mod github;
#[cfg(target_os = "windows")]
pub mod minidump;
pub mod output_console;
pub mod part_order;
pub mod puffin_profiler;
pub mod quality_check;
pub mod resource_mon;
pub mod scripts;
pub mod seb_lookup;
pub mod system_information;
pub mod tur_sheet;
pub mod websockets;

impl MastertechContext {
    pub fn simple_demo_menu(&mut self, ui: &mut Ui) {
        ui.label("Secret menu... -.-");
    }

    pub fn file_browser_popup(&mut self, ui: &mut Ui) {
        let current_state = self.show_deferred_viewport.load(Ordering::Relaxed);
        let new_state = !current_state; // Toggle the state: if it's true, make it false, and vice versa

        if current_state {
            if ui.button("Attach File Browser").clicked() {
                self.show_deferred_viewport
                    .store(new_state, Ordering::Relaxed);
            }
        } else {
            if ui.button("Detach File Browser").clicked() {
                self.show_deferred_viewport
                    .store(new_state, Ordering::Relaxed);
            }
        }
    }

    pub fn websocket_menu(&mut self, ui: &mut Ui) {
        let current_state = self.show_ws_viewport.load(Ordering::Relaxed);
        let new_state = !current_state; // Toggle the state: if it's true, make it false, and vice versa

        if current_state {
            if ui.button("Attach Websocket Console").clicked() {
                self.show_ws_viewport.store(new_state, Ordering::Relaxed);
            }
        } else {
            if ui.button("Detach Websocket Console").clicked() {
                self.show_ws_viewport.store(new_state, Ordering::Relaxed);
            }
        }
    }
}

impl TabViewer for MastertechContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "TUR Sheet" => self.tur_sheet(ui),
            "Console" => self.output_console(ui),
            "Part Order" => self.special_part_order(ui),
            "Scripts" => self.scripts(ui),
            "ToolBox" => self.shared_ctx.filesystem.display(ui),
            "File Browser 📂" => self.file_browse(ui),
            "SysInfo" => self.system_information(ui),
            #[cfg(target_os = "windows")]
            "Minidump Analysis" => self.mini_dump(ui),
            "QC ☑️" => self.quality_check(ui),
            "My Tasks" => self.shared_ctx.my_tasks(ui),
            "Ai" => self.shared_ctx.ai_playground(ui),
            "Store Tasks" => self.shared_ctx.store_tasks(ui),
            "Completed Tasks" => self.shared_ctx.completed_tasks(ui),
            "Bug Tracker" => self.github(ui),
            "Websockets" => self.websockets(ui),
            "Downloads" => self.downloads_page(ui),
            "SEB Lookup" => self.seb_lookup(ui),
            "Store Stock" => self.shared_ctx.stock_viewer(ui),
            "Logs" => logger_ui().show(ui),
            "Company Stock" => self.shared_ctx.stock_quantities_viewer(ui),
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
            "TUR Sheet" => self.simple_demo_menu(ui),
            "Websockets" => self.websocket_menu(ui),
            "File Browser 📂" => self.file_browser_popup(ui),
            _ => {
                ui.label(tab.to_string());
                ui.label("This is a context menu");
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

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &eframe::egui::Response) {
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
                        spawn(async move {
                            match get_stock(stock_tx.clone(), store_selection).await{
                                Ok(_) => info!("get_stock ran ok"),
                                Err(e) => error!("Error getting Stock: {e:?}")
                            }
                        });
                    }
                },
                "Company Stock" => {
                    let ex_stock_tx = self.shared_ctx.extra_stock_channel.0.clone();
                    spawn(async move {

                        match get_extra_stock_info(ex_stock_tx).await{
                            Ok(_) => info!("get_extra_stock_info ran ok"),
                            Err(e) => error!("Error getting Extra Stock info: {e:?}")
                        }
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
                        
                        spawn(async move {
                            let get_completed_tasks_for_store = get_completed_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_completed_tasks_for_store: {get_completed_tasks_for_store:?}");
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
                        let store_sel = self.shared_ctx.store_selection;
                        let store_selection = std::convert::Into::<Store>::into(store_sel).as_str().to_string();
                        
                        spawn(async move {
                            let get_tasks_for_store = get_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_tasks_for_store: {get_tasks_for_store:?}");
                        });
                    }
                },
                "Downloads" => {
                    let github_tx = self.github_releases_channel.0.clone();
                    let client = self.client.clone();
                    spawn(async move {
                        match get_github_releases(github_tx, client).await {
                            Ok(_) => info!("get_github_releases ran ok"),
                            Err(e) => error!("Error getting github releases: {e:?}"),
                        }
        
                        Ok::<(), Error>(())
                    });
                }
                _ => {}
            }
            
        }

    }
}

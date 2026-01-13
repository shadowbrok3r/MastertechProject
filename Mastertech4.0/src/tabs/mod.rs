use database::schema::{utilities::{get_completed_tasks_for_store, get_tasks_for_store}, FilterLiveTasks, Store};
use egui_dock::{tab_viewer::OnCloseResponse, NodeIndex, SurfaceIndex, TabViewer};
use crate::app_state::MastertechContext;
use eframe::egui::{Ui, WidgetText};
use github::get_github_releases;
use std::sync::atomic::Ordering;
use log::{error, info};
use egui::Color32;
use anyhow::Error;
use tokio::spawn;

pub mod file_browser;
pub mod github;
#[cfg(target_os = "windows")]
pub mod minidump;
pub mod part_order;
pub mod puffin_profiler;
pub mod quality_check;
pub mod resource_mon;
pub mod scripts;
pub mod system_information;
pub mod tur_sheet;
pub mod websockets;
pub mod egui_file_dialog;

// pub const TABS: [&str; 16] = [
//     "Lil menu",
//     "My Tools",
//     "Store Tasks",
//     "My Tasks",
//     "Ai",
//     "Admin Console",
//     "Completed Tasks",
//     "Bug Report",
//     "Logs",
//     "Database Editor",
//     "Json Viewer",
//     "Inventory",
//     "SEB Lookup",
//     "Task Audit",
//     "Customers",
// ];


impl MastertechContext {
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
            "Part Order" => self.special_part_order(ui),
            "KOTH" => self.shared_ctx.koth.ui(ui),
            "Scripts" => self.scripts(ui),
            "My Tools" => self.shared_ctx.filesystem.display(ui),
            "File Browser 📂" => self.file_browse(ui),
            "SysInfo" => self.system_information(ui),
            #[cfg(target_os = "windows")]
            "Minidump Analysis" => self.mini_dump(ui),
            "QC ☑️" => self.quality_check(ui),
            "Ai" => self.shared_ctx.ai_playground(ui),
            "Store Tasks" => self.shared_ctx.render_layout(ui, "Store Tasks"),
            "My Tasks" => self.shared_ctx.render_layout(ui, "My Tasks"),
            "Completed Tasks" => self.shared_ctx.render_layout(ui, "Completed Tasks"),
            "Bug Tracker" => self.github(ui),
            "Websockets" => self.websockets(ui),
            "Downloads" => self.downloads_page(ui),
            "Task Audit" => self.shared_ctx.task_table_viewer(ui, self.shared_ctx.ui_actions_tx.clone()),
            "Inventory" => self.shared_ctx.stock_tables.ui(ui),
            "Sales Tracker" => self.shared_ctx.sales_tracker.ui(ui),
            "Web Console" => self.shared_ctx.web_console.ui(ui),
            "Logs" => egui_logger::logger_ui()
                .log_levels([true, true, true, false, false])
                .warn_color(Color32::from_rgb(94, 215, 221)) 
                .error_color(Color32::from_rgb(255, 55, 102))
                .enable_category("eframe".to_string(), false)
                .enable_category("eframe::native::glow_integration".to_string(), false)
                .enable_category("egui_glow::shader_version".to_string(), false)
                .enable_category("egui_glow::painter".to_string(), false)
                .show(ui),
            "Resource Monitor" => self.show_resource_monitor(ui),
            "Admin Console" => self.shared_ctx.admin_console(ui),
            "Query Editor" => if let Some(usr) = &self.shared_ctx.current_user {
                if usr.is_admin() {
                    self.shared_ctx.query_editor.ui(ui)
                } else {
                    return;
                }
            } else {
                return;
            },
            "Create Prestashop Order" => self.shared_ctx.prestashop_order_form.ui(ui),
            "Threads" => self.shared_ctx.user_chat.ui(ui),
            _ => {}
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false] // No scroll bars by default
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface_index: SurfaceIndex,
        _node_index: NodeIndex,
    ) {
        match tab.as_str() {
            "Admin Console" => self.websocket_menu(ui),
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

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        self.open_tabs.remove(tab);
        OnCloseResponse::Close
    }

    fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &eframe::egui::Response) {
        if response.clicked() {
            match tab.as_str() {
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

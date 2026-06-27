use database::schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, FilterLiveTasks, Store};
use crate::{app_state::SharedContext, PlatformSpawner, Spawner};
use eframe::egui::*;
use log::info;

pub mod dock_session;
pub mod tab_id;
pub mod tasks;
pub mod task_audit;
pub mod stock;
pub mod ai_playground;
pub mod resource_monitor;
pub mod admin_console;
pub mod script_editor;
pub mod scripts;
pub mod user_chat;
pub mod database_viewer;
pub mod github;
pub mod raw_queries;
pub mod koth;
pub mod presta_order;
pub mod checkin_form;
pub mod sales_tracker;
pub mod web_console;
pub mod fleet_dashboard;
pub mod stress_lab;
#[cfg(not(target_arch = "wasm32"))]
pub mod plugins_tab;

pub use dock_session::{default_dock_session_native, default_dock_session_wasm, DockSession};
pub use tab_id::{TabContext, TabId, WAREHOUSE_DEFAULT_OPEN};

impl SharedContext {
    pub fn tab_context(&self) -> TabContext {
        TabContext::for_user(
            self.current_user
                .as_ref()
                .map(|u| u.is_warehouse())
                .unwrap_or(false),
        )
    }

    pub fn store_selection_menu(&mut self, ui: &mut Ui) {
        let selected = &mut self.store_selection;
        let current = selected.clone();
        let selected_text = Store::from_presta_store_id(&selected.to_string());

        ComboBox::new("Store_Selection", "")
            .selected_text(selected_text.as_str())
            .show_ui(ui, |ui| {
                ui.selectable_value(selected, Store::RIV.into_store_id() as u64, Store::RIV.as_str());
                ui.selectable_value(selected, Store::LTN.into_store_id() as u64, Store::LTN.as_str());
                ui.selectable_value(selected, Store::MUR.into_store_id() as u64, Store::MUR.as_str());
                ui.selectable_value(selected, Store::ORE.into_store_id() as u64, Store::ORE.as_str());
                ui.selectable_value(selected, Store::SAN.into_store_id() as u64, Store::SAN.as_str());
            });

        if *selected != current {
            let tasks_tx = self.initial_tasks_tx.clone();
            let store_users_tx = self.store_users_tx.clone();
            let store_selection = Store::from_presta_store_id(&selected.to_string());

            self.pending_store = Some(store_selection.clone());
            self.store_users.clear();
            self.tasks.clear();
            self.layout_configs = None;
            info!("Switching to store: {:?}", store_selection.as_str());
            PlatformSpawner::spawn(async move {
                let store_tasks = get_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                let tasks = get_completed_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                let get_store_users = get_store_users(store_users_tx, store_selection).await;
                info!("get_completed_tasks_for_store: {tasks:?}");
                info!("get_tasks_for_store: {store_tasks:?}");
                info!("get_store_users: {get_store_users:?}");
            });
        }
    }
}

impl egui_dock::TabViewer for SharedContext {
    type Tab = TabId;

    fn ui(&mut self, ui: &mut egui_dock::egui::Ui, tab: &mut Self::Tab) {
        let is_admin = self
            .current_user
            .as_ref()
            .map(|u| u.is_admin())
            .unwrap_or(false);
        let ctx = self.tab_context();
        match *tab {
            TabId::MyTasks => self.render_layout(ui, TabId::MyTasks.layout_page_name()),
            TabId::StoreTasks => self.render_layout(ui, TabId::StoreTasks.layout_page_name()),
            TabId::CompletedTasks => {
                self.render_layout(ui, TabId::CompletedTasks.layout_page_name())
            }
            TabId::SalesTracker => self.sales_tracker.ui(ui),
            TabId::Inventory => self.stock_tables.ui(ui),
            TabId::TaskAudit => self.task_table_viewer(ui, self.ui_actions_tx.clone()),
            TabId::Threads => self.user_chat.ui(ui),
            TabId::BugReport => self.github(ui),
            TabId::MyTools | TabId::FileBrowser => self.filesystem.display(ui),
            TabId::FleetDashboard => {
                if is_admin {
                    self.fleet_dashboard(ui);
                }
            }
            TabId::Logs => crate::ui_tools::egui_logger::logger_ui()
                .warn_color(Color32::from_rgb(94, 215, 221))
                .error_color(Color32::from_rgb(255, 55, 102))
                .log_levels([true, true, true, false, false])
                .enable_category("eframe".to_string(), false)
                .enable_category("eframe::native::glow_integration".to_string(), false)
                .enable_category("egui_glow::shader_version".to_string(), false)
                .enable_category("egui_glow::painter".to_string(), false)
                .show(ui),
            TabId::AdminConsole => self.admin_console(ui),
            TabId::WebConsole => self.web_console.ui(ui),
            TabId::Ai => {
                // Local self-diagnosis: chat about THIS machine via the
                // in-process Mastertech MCP tools, no remote client.
                self.enhanced_ai_playground.self_diagnosis = true;
                self.enhanced_ai_playground.focused_client = None;
                self.enhanced_ai_playground.enhanced_ai_playground(ui);
                let _ = self.enhanced_ai_playground.take_close_request();
            }
            TabId::DatabaseEditor => self.database_viewer.ui(ui, self.current_user.clone()),
            TabId::QueryEditor => {
                if is_admin {
                    self.query_editor.ui(ui);
                }
            }
            TabId::Koth => self.koth.ui(ui),
            TabId::CreatePrestashopOrder => self.prestashop_order_form.ui(ui),
            TabId::StressLab => self.stress_lab.ui(ui),
            TabId::ResourceMonitor => self.resource_mon.display(ui),
            TabId::Scripts => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label("Scripts are available in the desktop application.");
                });
            }
            #[cfg(not(target_arch = "wasm32"))]
            TabId::Plugins => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label("Plugin management is available in the desktop application.");
                });
            }
            #[cfg(target_arch = "wasm32")]
            TabId::Plugins => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label("Plugin management is not available in web mode.");
                });
            }
            _ => {
                let _ = ctx;
            }
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface_index: egui_dock::SurfaceIndex,
        _node_index: egui_dock::NodeIndex,
    ) {
        let ctx = self.tab_context();
        match *tab {
            TabId::StoreTasks => self.store_selection_menu(ui),
            _ => {
                ui.label(tab.title(ctx));
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title(self.tab_context()).into()
    }

    fn on_close(&mut self, _tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn on_add(&mut self, surface_index: egui_dock::SurfaceIndex, node_index: egui_dock::NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn add_popup(
        &mut self,
        ui: &mut Ui,
        surface_index: egui_dock::SurfaceIndex,
        node_index: egui_dock::NodeIndex,
    ) {
        ui.set_width(100.0);
        let tab_ctx = self.tab_context();
        for &tab in TabId::visible_for(tab_ctx) {
            let label = tab.title(tab_ctx);
            let open = self.dock.is_open(tab);
            if ui.selectable_label(open, label).clicked() {
                if !open {
                    self.on_add(surface_index, node_index);
                    self.pending_tab_adds
                        .push((surface_index, node_index, tab));
                } else {
                    self.pending_tab_removes.push(tab);
                }
                ui.close_kind(UiKind::Menu);
            }
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &Response) {
        if response.clicked() {
            match *tab {
                TabId::CompletedTasks => {
                    if self.tasks.filter_by_completion(true).is_empty() {
                        let tasks_tx = self.initial_tasks_tx.clone();
                        let store_sel = self.store_selection;
                        let store_selection =
                            Store::from_presta_store_id(&store_sel.to_string())
                                .as_str()
                                .to_string();
                        log::info!("Pulling completed tasks for store: {store_selection}");
                        PlatformSpawner::spawn(async move {
                            let get_completed_tasks_for_store =
                                get_completed_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_completed_tasks_for_store: {get_completed_tasks_for_store:?}");
                        });
                        self.task_layouts
                            .iter_mut()
                            .filter(|(page, _)| *page == "Completed Tasks")
                            .for_each(|(_, layout)| {
                                layout.loading = true;
                            });
                    }
                }
                TabId::StoreTasks => {
                    if self.tasks.filter_by_completion(false).is_empty() {
                        let tasks_tx = self.initial_tasks_tx.clone();
                        let store_sel = self.store_selection;
                        let store_selection =
                            Store::from_presta_store_id(&store_sel.to_string())
                                .as_str()
                                .to_string();
                        log::info!("Pulling tasks for store: {store_selection}");
                        PlatformSpawner::spawn(async move {
                            let get_tasks_for_store =
                                get_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_tasks_for_store: {get_tasks_for_store:?}");
                        });
                    }
                }
                TabId::StressLab => self.stress_lab.refresh_on_open(),
                _ => {}
            }
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }
}

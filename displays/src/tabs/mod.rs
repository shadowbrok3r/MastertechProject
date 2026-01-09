use database::schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, FilterLiveTasks, Store};
use crate::{app_state::SharedContext, PlatformSpawner, Spawner};
use eframe::egui::*;
use log::info;

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

pub enum Tabs {
    #[cfg(not(target_arch="wasm32"))]
    Mastertech(MastertechTabs),
    Shared(SharedTabs)
}

pub enum MastertechTabs {
    TurSheet,
    PartOrder,
    Scripts,
    FileBrowser,
    SysInfo,
    MinidumpAnalysis,
    Qc,
    Ai,
    Websockets,
    Downloads,
}

pub enum SharedTabs {
    MyTasks,
    StoreTasks,
    CompletedTasks,
    SalesTracker,
    Inventory,
    TaskAudit,
    Threads,
    BugReport,
    MyTools,
    AdminConsole,
    WebConsole,
    DatabaseEditor,
    ResourceMonitor,
    QueryEditor,
    Koth,
    CreatePrestashopOrder,
    Logs,
}

impl Tabs {
    pub fn as_str(&self) -> &str {
        match self {
            #[cfg(not(target_arch="wasm32"))]
            Tabs::Mastertech(mastertech_tabs) => {
                match mastertech_tabs {
                    MastertechTabs::TurSheet => "Tur Sheet",
                    MastertechTabs::PartOrder => "Part Order",
                    MastertechTabs::Scripts => "Scripts",
                    MastertechTabs::FileBrowser => "File Browser 📂",
                    MastertechTabs::SysInfo => "SysInfo",
                    MastertechTabs::MinidumpAnalysis => "Minidump Analysis",
                    MastertechTabs::Qc => "Qc",
                    MastertechTabs::Ai => "Ai",
                    MastertechTabs::Websockets => "Websockets",
                    MastertechTabs::Downloads => "Downloads",
                }
            },
            Tabs::Shared(shared_tabs) => {
                match shared_tabs {
                    SharedTabs::MyTasks => "My Tasks",
                    SharedTabs::StoreTasks => "Store Tasks",
                    SharedTabs::CompletedTasks => "Completed Tasks",
                    SharedTabs::SalesTracker => "Sales Tracker",
                    SharedTabs::Inventory => "Inventory",
                    SharedTabs::TaskAudit => "TaskAudit",
                    SharedTabs::Threads => "Threads",
                    SharedTabs::BugReport => "BugReport",
                    SharedTabs::MyTools => "My Tools",
                    SharedTabs::AdminConsole => "Admin Console",
                    SharedTabs::WebConsole => "Web Console",
                    SharedTabs::DatabaseEditor => "Database Editor",
                    SharedTabs::ResourceMonitor => "Resource Monitor",
                    SharedTabs::QueryEditor => "Query Editor",
                    SharedTabs::Koth => "Koth",
                    SharedTabs::CreatePrestashopOrder => "Create Prestashop Order",
                    SharedTabs::Logs => "Logs",
                }
            },
        }
    }

    pub fn from_str(tab: &str) -> Self {
        match tab {
            "My Tasks" => Self::Shared(SharedTabs::MyTasks),
            "Store Tasks" => Self::Shared(SharedTabs::StoreTasks),
            "Completed Tasks" => Self::Shared(SharedTabs::CompletedTasks),
            "Database Editor" => Self::Shared(SharedTabs::DatabaseEditor),
            "KOTH" => Self::Shared(SharedTabs::Koth),
            "My Tools" => Self::Shared(SharedTabs::MyTools),
            "Task Audit" => Self::Shared(SharedTabs::TaskAudit),
            "Inventory" => Self::Shared(SharedTabs::Inventory),
            "Sales Tracker" => Self::Shared(SharedTabs::SalesTracker),
            "Logs" => Self::Shared(SharedTabs::Logs),
            "Resource Monitor" => Self::Shared(SharedTabs::ResourceMonitor),
            "Admin Console" => Self::Shared(SharedTabs::AdminConsole),
            "Web Console" => Self::Shared(SharedTabs::WebConsole),
            "Query Editor" => Self::Shared(SharedTabs::QueryEditor),
            "Create Prestashop Order" => Self::Shared(SharedTabs::CreatePrestashopOrder),
            "Threads" => Self::Shared(SharedTabs::Threads),
            "Bug Tracker" => Self::Shared(SharedTabs::BugReport),
            #[cfg(not(target_arch="wasm32"))]
            "TUR Sheet" => Self::Mastertech(MastertechTabs::TurSheet),
            #[cfg(not(target_arch="wasm32"))]
            "Part Order" => Self::Mastertech(MastertechTabs::PartOrder),
            #[cfg(not(target_arch="wasm32"))]
            "Scripts" => Self::Mastertech(MastertechTabs::Scripts),
            #[cfg(not(target_arch="wasm32"))]
            "File Browser 📂" => Self::Mastertech(MastertechTabs::FileBrowser),
            #[cfg(not(target_arch="wasm32"))]
            "SysInfo" => Self::Mastertech(MastertechTabs::SysInfo),
            #[cfg(not(target_arch="wasm32"))]
            "Minidump Analysis" => Self::Mastertech(MastertechTabs::MinidumpAnalysis),
            #[cfg(not(target_arch="wasm32"))]
            "QC ☑️" => Self::Mastertech(MastertechTabs::Qc),
            #[cfg(not(target_arch="wasm32"))]
            "Ai" => Self::Mastertech(MastertechTabs::Ai),
            #[cfg(not(target_arch="wasm32"))]
            "Websockets" => Self::Mastertech(MastertechTabs::Websockets),
            #[cfg(not(target_arch="wasm32"))]
            "Downloads" => Self::Mastertech(MastertechTabs::Downloads),
            &_ => Self::Shared(SharedTabs::MyTasks),
        }

    }
}

#[cfg(target_arch="wasm32")]
pub const TABS: [&str; 16] = [
    "My Tasks",
    "Store Tasks",
    "Completed Tasks",
    "Sales Tracker",
    "Inventory",
    "Task Audit",
    "Threads",
    "Bug Report",
    "My Tools",
    "Logs",
    "Admin Console",
    "Web Console",
    "Database Editor",
    "Query Editor",
    "KOTH",
    "Create Prestashop Order",
];

#[cfg(not(target_arch="wasm32"))]
pub const TABS: [&str; 26] = [
    "TUR Sheet",
    "Part Order",
    "KOTH",
    "Scripts",
    "My Tools",
    "File Browser 📂",
    "SysInfo",
    "Minidump Analysis",
    "QC ☑️",
    "Ai",
    "Store Tasks",
    "My Tasks",
    "Completed Tasks",
    "Bug Tracker",
    "Websockets",
    "Downloads",
    "Task Audit",
    "Inventory",
    "Sales Tracker",
    "Logs",
    "Resource Monitor",
    "Admin Console",
    "Web Console",
    "Query Editor",
    "Create Prestashop Order",
    "Threads",
];

impl SharedContext {
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

            // Signal store switch
            self.pending_store = Some(store_selection.clone());
            self.store_users.clear();
            self.tasks.clear();
            self.layout_configs = None; // Force reinitialization
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
    type Tab = String;

    fn ui(&mut self, ui: &mut egui_dock::egui::Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "My Tasks" => self.render_layout(ui, "My Tasks"),
            "Store Tasks" => self.render_layout(ui, "Store Tasks"),
            "Completed Tasks" => self.render_layout(ui, "Completed Tasks"),
            "Sales Tracker" => self.sales_tracker.ui(ui),
            "Inventory" => self.stock_tables.ui(ui),
            "Task Audit" => self.task_table_viewer(ui, self.ui_actions_tx.clone()),
            "Threads" => self.user_chat.ui(ui),
            "Bug Report" => self.github(ui),
            "My Tools" => self.filesystem.display(ui),
            "Logs" => egui_logger::logger_ui()
                .warn_color(Color32::from_rgb(94, 215, 221)) 
                .error_color(Color32::from_rgb(255, 55, 102)) 
                .log_levels([true, true, true, false, false])
                // there should be a way to set default false...
                .enable_category("eframe".to_string(), false)
                .enable_category("eframe::native::glow_integration".to_string(), false)
                .enable_category("egui_glow::shader_version".to_string(), false)
                .enable_category("egui_glow::painter".to_string(), false)
                .show(ui),
            "Admin Console" => self.admin_console(ui),
            "Web Console" => self.web_console.ui(ui),
            "Database Editor" => self.database_viewer.ui(ui, self.current_user.clone()),
            "Query Editor" => if let Some(usr) = &self.current_user {
                if usr.is_admin() {
                    self.query_editor.ui(ui)
                } else {
                    return;
                }
            } else {
                return;
            },
            "KOTH" => self.koth.ui(ui),
            "Create Prestashop Order" => self.prestashop_order_form.ui(ui),
            // "Ai" => self.ai_playground(ui),
            _ => {}
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface_index: egui_dock::SurfaceIndex,
        _node_index: egui_dock::NodeIndex,
    ) {
        match tab.as_str() {
            "Store Tasks" => self.store_selection_menu(ui),
            _ => {
                ui.label(tab.as_str());
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        self.open_tabs.remove(tab.as_str());
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn on_add(&mut self, surface_index: egui_dock::SurfaceIndex, node_index: egui_dock::NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn add_popup(&mut self, ui: &mut Ui, surface_index: egui_dock::SurfaceIndex, node_index: egui_dock::NodeIndex) {
        ui.set_width(100.0);
        for tab in TABS {
            if ui
                .selectable_label(self.open_tabs.contains(tab), tab)
                .clicked()
            {
                // Queue the add/remove to be applied after DockArea::show
                if !self.open_tabs.contains(tab) {
                    self.on_add(surface_index, node_index);
                    self.pending_tab_adds.push((surface_index, node_index, tab.to_string()));
                } else {
                    // Toggle off: request remove by name
                    self.pending_tab_removes.push(tab.to_string());
                }
                ui.close_kind(UiKind::Menu);
            }
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &Response) {
        if response.clicked() {
            match tab.as_str() {
                "Completed Tasks" => {
                    // First, make sure there are no completed tasks loaded.
                    // If there no completed tasks, then load them.
                    // Otherwise, make sure the tasks that are completed 
                    // are for the correct selected store. 
                    if self.tasks.filter_by_completion(true).is_empty() {
                        let tasks_tx = self.initial_tasks_tx.clone();
                        let store_sel = self.store_selection;
                        let store_selection = Store::from_presta_store_id(&store_sel.to_string()).as_str().to_string();
                        log::info!("Pulling completed tasks for store: {store_selection}");
                        PlatformSpawner::spawn(async move {
                            let get_completed_tasks_for_store = get_completed_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_completed_tasks_for_store: {get_completed_tasks_for_store:?}");
                        });
                        self.task_layouts
                            .iter_mut()
                            .filter(|(page, _)| *page == "Completed Tasks")
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
                    if self.tasks.filter_by_completion(false).is_empty() {
                        let tasks_tx = self.initial_tasks_tx.clone();
                        let store_sel = self.store_selection;
                        let store_selection = Store::from_presta_store_id(&store_sel.to_string()).as_str().to_string();
                        log::info!("Pulling tasks for store: {store_selection}");
                        PlatformSpawner::spawn(async move {
                            let get_tasks_for_store = get_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_tasks_for_store: {get_tasks_for_store:?}");
                        });
                    }
                    
                },
                _ => {}
            }   
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false] // No scroll bars by default
    }
}

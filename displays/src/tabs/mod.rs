use database::schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, Store};
use crate::{app_state::SharedContext, PlatformSpawner, Spawner};
use eframe::egui::{ComboBox, Ui};
use log::info;

pub mod tasks;
pub mod task_audit;
pub mod stock;
pub mod ai_playground;
pub mod resource_monitor;
pub mod admin_console;
pub mod script_editor;
pub mod user_chat;
pub mod database_viewer;
pub mod github;
pub mod raw_queries;
pub mod koth;
pub mod presta_order;
pub mod checkin_form;
pub mod sales_tracker;

#[cfg(target_arch="wasm32")]
pub const TABS: [&str; 11] = [
    "My Tasks",
    "Store Tasks",
    "Completed Tasks",
    "Inventory",
    "Task Audit",
    "Threads",
    "Bug Report",
    "My Tools",
    "Logs",
    "Admin Console",
    "Database Editor",
    // "Ai",
    // "Json Viewer",
    // "Customers",
];

#[cfg(not(target_arch="wasm32"))]
pub const TABS: [&str; 25] = [
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
    "Query Editor",
    "Create Prestashop Order",
    "Threads",
];

impl SharedContext {
    pub fn store_selection_menu(&mut self, ui: &mut Ui) {
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
            let tasks_tx = self.initial_tasks_tx.clone();
            let store_users_tx = self.store_users_tx.clone();
            let store_selection = std::convert::Into::<Store>::into(*selected);

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


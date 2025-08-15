use database::schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, FilterLiveTasks, Store};

use eframe::egui::{Color32, ComboBox, Response, Ui, UiKind, WidgetText};
use egui_dock::{tab_viewer::OnCloseResponse, NodeIndex, SurfaceIndex, TabViewer};
use super::app_state::MtechServerContext;
use wasm_bindgen_futures::spawn_local;
use log::info;

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

impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "My Tasks" => self.shared_ctx.render_layout(ui, "My Tasks"),
            "Store Tasks" => self.shared_ctx.render_layout(ui, "Store Tasks"),
            "Completed Tasks" => self.shared_ctx.render_layout(ui, "Completed Tasks"),
            "Inventory" => self.shared_ctx.stock_tables.ui(ui),
            "Task Audit" => self.shared_ctx.task_table_viewer(ui),
            "Threads" => self.shared_ctx.user_chat.ui(ui),
            "Bug Report" => self.shared_ctx.github(ui),
            "My Tools" => self.shared_ctx.filesystem.display(ui),
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
            "Admin Console" => self.shared_ctx.admin_console(ui),
            "Database Editor" => self.shared_ctx.database_viewer.ui(ui, self.shared_ctx.current_user.clone()),
            "Query Editor" => if let Some(usr) = &self.shared_ctx.current_user {
                if usr.is_admin() {
                    self.shared_ctx.query_editor.ui(ui)
                } else {
                    return;
                }
            } else {
                return;
            },
            "KOTH" => self.shared_ctx.koth.ui(ui),
            "Create Prestashop Order" => self.shared_ctx.prestashop_order_form.ui(ui),
            // "Ai" => self.shared_ctx.ai_playground(ui),
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

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        self.open_tabs.remove(tab);
        OnCloseResponse::Close
    }

    fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn add_popup(&mut self, ui: &mut Ui, surface_index: SurfaceIndex, node_index: NodeIndex) {
        ui.set_width(100.0);
        for tab in TABS {
            if ui
                .selectable_label(self.open_tabs.contains(tab), tab)
                .clicked()
            {
                if !self.open_tabs.contains(tab) {
                    self.on_add(surface_index, node_index);
                    // if let Some(index) = self.shared_ctx.tree.find_tab(&tab.to_string()) {
                    //     self.shared_ctx.tree.remove_tab(index);
                    //     self.open_tabs.remove(tab);
                    // } else {
                    //     self.open_tabs.insert(tab.to_string());
                    //     self.shared_ctx.tree.push_to_focused_leaf(tab.to_string());
                    // }
                }
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
                    if self.shared_ctx.tasks.filter_by_completion(false).is_empty() {
                        let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
                        let store_sel = self.shared_ctx.store_selection;
                        let store_selection = std::convert::Into::<Store>::into(store_sel).as_str().to_string();
                        
                        spawn_local(async move {
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


impl MtechServerContext {
    pub fn simple_demo_menu(&mut self, ui: &mut Ui) {
        if ui.button("Open...").clicked() {
            ui.close_kind(UiKind::Menu);
        }
        ui.menu_button("SubMenu", |ui| {
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_kind(UiKind::Menu);
                }
                let _ = ui.button("Item");
            });
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_kind(UiKind::Menu);
                }
                let _ = ui.button("Item");
            });
            let _ = ui.button("Item");
            if ui.button("Open...").clicked() {
                ui.close_kind(UiKind::Menu);
            }
        });
        ui.menu_button("SubMenu", |ui| {
            let _ = ui.button("Item1");
            let _ = ui.button("Item2");
            let _ = ui.button("Item3");
            let _ = ui.button("Item4");
            if ui.button("Open...").clicked() {
                ui.close_kind(UiKind::Menu);
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
            let store_selection = std::convert::Into::<Store>::into(*selected);

            // Signal store switch
            self.shared_ctx.pending_store = Some(store_selection.clone());
            self.shared_ctx.store_users.clear();
            self.shared_ctx.tasks.clear();
            self.shared_ctx.layout_configs = None; // Force reinitialization
            info!("Switching to store: {:?}", store_selection.as_str());
            spawn_local(async move {
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
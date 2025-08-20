// use database::schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, FilterLiveTasks, Store};
// use displays::tabs::TABS;
// use egui_dock::{tab_viewer::OnCloseResponse, NodeIndex, SurfaceIndex, TabViewer};
// use eframe::egui::{Color32, ComboBox, Response, Ui, UiKind, WidgetText};

// use wasm_bindgen_futures::spawn_local;
// use log::info;


// impl TabViewer for MtechServerContext {
//     type Tab = String;

//     fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
//         match tab.as_str() {
//             "My Tasks" => self.shared_ctx.render_layout(ui, "My Tasks"),
//             "Store Tasks" => self.shared_ctx.render_layout(ui, "Store Tasks"),
//             "Completed Tasks" => self.shared_ctx.render_layout(ui, "Completed Tasks"),
//             "Sales Tracker" => self.shared_ctx.sales_tracker.ui(ui),
//             "Inventory" => self.shared_ctx.stock_tables.ui(ui),
//             "Task Audit" => self.shared_ctx.task_table_viewer(ui),
//             "Threads" => self.shared_ctx.user_chat.ui(ui),
//             "Bug Report" => self.shared_ctx.github(ui),
//             "My Tools" => self.shared_ctx.filesystem.display(ui),
//             "Logs" => egui_logger::logger_ui()
//                 .warn_color(Color32::from_rgb(94, 215, 221)) 
//                 .error_color(Color32::from_rgb(255, 55, 102)) 
//                 .log_levels([true, true, true, false, false])
//                 // there should be a way to set default false...
//                 .enable_category("eframe".to_string(), false)
//                 .enable_category("eframe::native::glow_integration".to_string(), false)
//                 .enable_category("egui_glow::shader_version".to_string(), false)
//                 .enable_category("egui_glow::painter".to_string(), false)
//                 .show(ui),
//             "Admin Console" => self.shared_ctx.admin_console(ui),
//             "Database Editor" => self.shared_ctx.database_viewer.ui(ui, self.shared_ctx.current_user.clone()),
//             "Query Editor" => if let Some(usr) = &self.shared_ctx.current_user {
//                 if usr.is_admin() {
//                     self.shared_ctx.query_editor.ui(ui)
//                 } else {
//                     return;
//                 }
//             } else {
//                 return;
//             },
//             "KOTH" => self.shared_ctx.koth.ui(ui),
//             "Create Prestashop Order" => self.shared_ctx.prestashop_order_form.ui(ui),
//             // "Ai" => self.shared_ctx.ai_playground(ui),
//             _ => {}
//         }
//     }

//     fn context_menu(
//         &mut self,
//         ui: &mut Ui,
//         tab: &mut Self::Tab,
//         _surface_index: SurfaceIndex,
//         _node_index: NodeIndex,
//     ) {
//         match tab.as_str() {
//             "My Tasks" => self.simple_demo_menu(ui),
//             "Store Tasks" => self.store_selection_menu(ui),
//             _ => {
//                 ui.label(tab.to_string());
//             }
//         }
//     }

//     fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
//         tab.as_str().into()
//     }

//     fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
//         self.open_tabs.remove(tab);
//         OnCloseResponse::Close
//     }

//     fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
//         self.added_nodes.push((surface_index, node_index));
//     }

//     fn add_popup(&mut self, ui: &mut Ui, surface_index: SurfaceIndex, node_index: NodeIndex) {
//         ui.set_width(100.0);
//         for tab in TABS {
//             if ui
//                 .selectable_label(self.open_tabs.contains(tab), tab)
//                 .clicked()
//             {
//                 if !self.open_tabs.contains(tab) {
//                     self.on_add(surface_index, node_index);
//                     // if let Some(index) = self.shared_ctx.tree.find_tab(&tab.to_string()) {
//                     //     self.shared_ctx.tree.remove_tab(index);
//                     //     self.open_tabs.remove(tab);
//                     // } else {
//                     //     self.open_tabs.insert(tab.to_string());
//                     //     self.shared_ctx.tree.push_to_focused_leaf(tab.to_string());
//                     // }
//                 }
//             }
//         }
//     }

//     fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &Response) {
//         if response.clicked() {
//             match tab.as_str() {
//                 "Completed Tasks" => {
//                     // First, make sure there are no completed tasks loaded.
//                     // If there no completed tasks, then load them.
//                     // Otherwise, make sure the tasks that are completed 
//                     // are for the correct selected store. 
//                     if self.shared_ctx.tasks.filter_by_completion(true).is_empty() {
//                         let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
//                         let store_sel = self.shared_ctx.store_selection;
//                         let store_selection = std::convert::Into::<Store>::into(store_sel).as_str().to_string();
                        
//                         spawn_local(async move {
//                             let get_completed_tasks_for_store = get_completed_tasks_for_store(tasks_tx, store_selection).await;
//                             info!("get_completed_tasks_for_store: {get_completed_tasks_for_store:?}");
//                         });
//                         self.shared_ctx.task_layouts
//                             .iter_mut()
//                             .filter(|(page, _)| *page == "Completed Tasks")
//                             .for_each(|(_, layout)| {
//                                 layout.loading = true;
//                         });
//                     }
                    
//                 },
//                 "Store Tasks" => {
//                     // First, make sure there are no store tasks loaded.
//                     // If there no store tasks, then load them.
//                     // Otherwise, make sure the tasks that are loaded 
//                     // for the selected store are for the CORRECT selected store. 
//                     if self.shared_ctx.tasks.filter_by_completion(false).is_empty() {
//                         let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
//                         let store_sel = self.shared_ctx.store_selection;
//                         let store_selection = std::convert::Into::<Store>::into(store_sel).as_str().to_string();
                        
//                         spawn_local(async move {
//                             let get_tasks_for_store = get_tasks_for_store(tasks_tx, store_selection).await;
//                             info!("get_tasks_for_store: {get_tasks_for_store:?}");
//                         });
//                     }
                    
//                 },
//                 _ => {}
//             }   
//         }
//     }

//     fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
//         [false, false] // No scroll bars by default
//     }
// }


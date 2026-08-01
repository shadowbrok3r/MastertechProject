use database::schema::{utilities::{get_completed_tasks_for_store, get_tasks_for_store}, FilterLiveTasks, Store};
use displays::tabs::{TabContext, TabId};
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
pub mod shopify_orders;
pub mod stress_test;
pub mod system_information;
pub mod tur_sheet;
pub mod egui_file_dialog;

impl MastertechContext {
    pub fn tab_context(&self) -> TabContext {
        TabContext::MastertechNative
    }

    pub fn file_browser_popup(&mut self, ui: &mut Ui) {
        let current_state = self.show_deferred_viewport.load(Ordering::Relaxed);
        let new_state = !current_state;

        if current_state {
            if ui.button("Attach File Browser").clicked() {
                self.show_deferred_viewport
                    .store(new_state, Ordering::Relaxed);
            }
        } else if ui.button("Detach File Browser").clicked() {
            self.show_deferred_viewport
                .store(new_state, Ordering::Relaxed);
        }
    }

    pub fn terminal_tab(&mut self, ui: &mut Ui) {
        // While detached, the viewport window owns the render; show a stub here.
        if self.show_terminal_viewport.load(Ordering::Relaxed) {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("Terminal detached to a separate window.");
                if ui.button("Re-attach").clicked() {
                    self.show_terminal_viewport.store(false, Ordering::Relaxed);
                }
            });
            return;
        }
        let user = self.shared_ctx.current_user.clone();
        self.embedded_terminal
            .get_or_insert_with(|| crate::terminal_mode::embedded::EmbeddedTerminal::new(user))
            .ui(ui);
    }

    pub fn terminal_popup(&mut self, ui: &mut Ui) {
        if self.show_terminal_viewport.load(Ordering::Relaxed) {
            if ui.button("Attach Terminal").clicked() {
                self.show_terminal_viewport.store(false, Ordering::Relaxed);
            }
        } else if ui.button("Detach Terminal").clicked() {
            self.show_terminal_viewport.store(true, Ordering::Relaxed);
        }
    }
}

impl TabViewer for MastertechContext {
    type Tab = TabId;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match *tab {
            TabId::TurSheet => self.tur_sheet(ui),
            TabId::PartOrder => self.special_part_order(ui),
            TabId::Koth => self.shared_ctx.koth.ui(ui),
            TabId::DatabaseEditor => {
                self.shared_ctx
                    .database_viewer
                    .ui(ui, self.shared_ctx.current_user.clone())
            }
            TabId::Scripts => self.scripts(ui),
            TabId::MyTools | TabId::FileBrowser => self.file_browse(ui),
            TabId::SysInfo | TabId::ResourceMonitor => self.show_resource_monitor(ui),
            #[cfg(target_os = "windows")]
            TabId::MinidumpAnalysis => self.mini_dump(ui),
            #[cfg(not(target_os = "windows"))]
            TabId::MinidumpAnalysis => {}
            TabId::Qc => self.quality_check(ui),
            TabId::Ai => {
                // Self-diagnosis chat about this machine via the in-process Mastertech MCP tools.
                self.shared_ctx.enhanced_ai_playground.self_diagnosis = true;
                self.shared_ctx.enhanced_ai_playground.focused_client = None;
                self.shared_ctx.enhanced_ai_playground.enhanced_ai_playground(ui);
                let _ = self.shared_ctx.enhanced_ai_playground.take_close_request();
            }
            TabId::StoreTasks => {
                self.shared_ctx
                    .render_layout(ui, TabId::StoreTasks.layout_page_name())
            }
            TabId::MyTasks => {
                self.shared_ctx
                    .render_layout(ui, TabId::MyTasks.layout_page_name())
            }
            TabId::CompletedTasks => {
                self.shared_ctx
                    .render_layout(ui, TabId::CompletedTasks.layout_page_name())
            }
            TabId::BugReport => self.github(ui),
            TabId::Downloads => self.downloads_page(ui),
            TabId::TaskAudit => self
                .shared_ctx
                .task_table_viewer(ui, self.shared_ctx.ui_actions_tx.clone()),
            TabId::Inventory => self.shared_ctx.stock_tables.ui(ui),
            TabId::SalesTracker => self.shared_ctx.sales_tracker.ui(ui),
            TabId::WebConsole => self.shared_ctx.web_console.ui(ui),
            TabId::Logs => displays::ui_tools::egui_logger::logger_ui()
                .log_levels([true, true, true, false, false])
                .warn_color(Color32::from_rgb(94, 215, 221))
                .error_color(Color32::from_rgb(255, 55, 102))
                .enable_category("eframe".to_string(), false)
                .enable_category("eframe::native::glow_integration".to_string(), false)
                .enable_category("egui_glow::shader_version".to_string(), false)
                .enable_category("egui_glow::painter".to_string(), false)
                .enable_category("evtx::evtx_chunk".to_string(), false)
                .enable_category("evtx::evtx_parser".to_string(), false)
                .show(ui),
            TabId::AdminConsole => self.shared_ctx.admin_console(ui),
            TabId::ServerConsole => self.shared_ctx.server_console.ui(ui),
            TabId::FleetDashboard => self.shared_ctx.fleet_dashboard(ui),
            TabId::StressLab => self.shared_ctx.stress_lab.ui(ui),
            TabId::StressTest => self.show_stress_test(ui),
            TabId::QueryEditor => {
                if let Some(usr) = &self.shared_ctx.current_user {
                    if usr.is_admin() {
                        self.shared_ctx.query_editor.ui(ui);
                    }
                }
            }
            TabId::CreatePrestashopOrder => self.shared_ctx.prestashop_order_form.ui(ui),
            TabId::ShopifyOrders => self.shopify_orders(ui),
            TabId::Threads => self.shared_ctx.user_chat.ui(ui),
            TabId::Plugins => {
                displays::tabs::plugins_tab::plugins_tab_ui(ui, &self.plugin_manager)
            }
            TabId::Terminal => self.terminal_tab(ui),
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface_index: SurfaceIndex,
        _node_index: NodeIndex,
    ) {
        match *tab {
            TabId::FileBrowser => self.file_browser_popup(ui),
            TabId::Terminal => self.terminal_popup(ui),
            _ => {
                ui.label(tab.title(self.tab_context()));
                ui.label("This is a context menu");
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title(self.tab_context()).into()
    }

    fn on_close(&mut self, _tab: &mut Self::Tab) -> OnCloseResponse {
        OnCloseResponse::Close
    }

    fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &eframe::egui::Response) {
        if response.clicked() {
            match *tab {
                TabId::CompletedTasks => {
                    if self
                        .shared_ctx
                        .tasks
                        .filter_by_completion(true)
                        .is_empty()
                    {
                        let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
                        let store_sel = self.shared_ctx.store_selection;
                        let store_selection =
                            std::convert::Into::<Store>::into(store_sel).as_str().to_string();

                        spawn(async move {
                            let get_completed_tasks_for_store =
                                get_completed_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_completed_tasks_for_store: {get_completed_tasks_for_store:?}");
                        });
                    }
                }
                TabId::StoreTasks => {
                    if self
                        .shared_ctx
                        .tasks
                        .filter_by_completion(false)
                        .is_empty()
                    {
                        let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
                        let store_sel = self.shared_ctx.store_selection;
                        let store_selection =
                            std::convert::Into::<Store>::into(store_sel).as_str().to_string();

                        spawn(async move {
                            let get_tasks_for_store =
                                get_tasks_for_store(tasks_tx, store_selection).await;
                            info!("get_tasks_for_store: {get_tasks_for_store:?}");
                        });
                    }
                }
                TabId::Downloads => {
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

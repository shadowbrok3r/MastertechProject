use crate::app_state::MastertechContext;
use crate::tabs::logger::logger_ui;
use eframe::egui::{Ui, WidgetText};
use egui_dock::{NodeIndex, SurfaceIndex, TabViewer};
use std::sync::atomic::Ordering;

pub mod file_browser;
pub mod github;
pub mod logger;
pub mod mastertech_website;
#[cfg(target_os = "windows")]
pub mod minidump;
pub mod output_console;
pub mod part_order;
pub mod puffin_profiler;
pub mod quality_check;
pub mod resource_mon;
pub mod scripts;
pub mod seb_lookup;
pub mod stock;
pub mod system_information;
pub mod toolbox;
pub mod tur_sheet;
pub mod websockets;
pub mod stock_quantities;

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
            "ToolBox" => self.toolbox(ui),
            "File Browser 📂" => self.file_browse(ui),
            "SysInfo" => self.system_information(ui),
            #[cfg(target_os = "windows")]
            "Minidump Analysis" => self.mini_dump(ui),
            "QC ☑️" => self.quality_check(ui),
            "Tasks" => self.mastertech_website(ui),
            "Bug Tracker" => self.github(ui),
            "Websockets" => self.websockets(ui),
            "Downloads" => self.downloads_page(ui),
            "SEB Lookup" => self.seb_lookup(ui),
            "Stock" => self.stock_viewer(ui),
            "Logs" => logger_ui().show(ui),
            "Stock Quantity" => self.stock_quantities_viewer(ui),
            _ => {
                let sysinfo_tab = &"SysInfo".to_string();
                if ui.label(tab.as_str()).clicked() {
                    if tab.as_str() == sysinfo_tab {
                        self.specs_first_run = true;
                    }
                };
            }
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
}

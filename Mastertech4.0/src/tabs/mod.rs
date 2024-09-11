use crate::app_state::MastertechContext;
use eframe::egui::Ui;
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
pub mod system_information;
pub mod toolbox;
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

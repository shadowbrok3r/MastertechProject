use eframe::egui::Ui;

use crate::tabs::admin_console::client_interface::WebSocketClient;



impl WebSocketClient {
    pub fn show_live_stats(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.vertical_centered_justified(|ui| {
                self.resource_monitor.display(ui);
            });
        });
        ui.add_space(10.0);
    }
}
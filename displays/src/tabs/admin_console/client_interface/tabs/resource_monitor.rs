use eframe::egui::Ui;

use crate::{tabs::{admin_console::client_interface::WebSocketClient, resource_monitor::process_table::ProcessAction}, Cmd};



impl WebSocketClient {
    pub fn show_live_stats(&mut self, ui: &mut Ui) {
        // Poll for process actions from context menu
        while let Some(action) = self.resource_monitor.process_table_viewer.try_recv_action() {
            match action {
                ProcessAction::Kill(pid) => {
                    log::info!("Sending kill process command for PID: {}", pid);
                    let _ = self.send_cmd_tx.try_send(Cmd::KillProcess(pid));
                }
                ProcessAction::OpenInExplorer(path) => {
                    log::info!("Sending open in explorer command for path: {}", path);
                    let _ = self.send_cmd_tx.try_send(Cmd::OpenProcessInExplorer(path));
                }
            }
        }
        
        ui.vertical_centered(|ui| {
            ui.vertical_centered_justified(|ui| {
                self.resource_monitor.display(ui);
            });
        });
        ui.add_space(10.0);
    }
}
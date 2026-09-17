use crate::app_state::MastertechContext;
use eframe::egui::Ui;

impl MastertechContext {
    /// Renders this machine through the same per-machine view the admin console
    /// uses for a remote client, over the in-process transport.
    pub fn show_resource_monitor(&mut self, ui: &mut Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::app_state::LocalMachineSession;

            let session = self
                .local_machine
                .get_or_insert_with(LocalMachineSession::open);

            // The local snapshot carries I/O rates, page-file figures and board
            // rails that the wire payload never does.
            session
                .view
                .resource_monitor
                .set_telemetry(crate::filesystem::system_info::current_telemetry_snapshot());

            session.view.show(ui);
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.shared_ctx.resource_mon.display(ui);
        }
    }
}

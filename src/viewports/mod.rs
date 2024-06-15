use std::sync::{atomic::Ordering, Arc};

use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportId};

use crate::app_state::MasterTechApp;

impl MasterTechApp{
    pub fn viewport_loader(&mut self, ctx: &Context) {
        if self.context.show_deferred_viewport.load(Ordering::Relaxed) {
            let file_browser_clone = Arc::clone(&self.context.file_browser);
            let show_deferred_viewport = self.context.show_deferred_viewport.clone();
            let viewport_id = ViewportId::from_hash_of("deferred_viewport");
            let viewport_builder = ViewportBuilder::default().with_title("File Browser").with_inner_size([400.0, 500.0]);
            ctx.show_viewport_deferred(viewport_id,viewport_builder,move |ctx, _class| {
                    CentralPanel::default().show(ctx, |ui| {
                        let (command_tx, command_rx) = crossbeam::channel::unbounded();
                        // Lock the Mutex and show the GUI
                        let mut file_browser = file_browser_clone.lock().unwrap();
                        file_browser.show(ui, command_tx, command_rx);
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        // Tell parent to close us.
                        show_deferred_viewport.store(false, Ordering::Relaxed);
                    }
                },
            );
        }
    }
}
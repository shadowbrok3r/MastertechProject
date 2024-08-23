use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportId};
use std::{sync::{atomic::Ordering, Arc}, time::Duration};
use crate::app_state::MasterTechApp;
use log::info;

impl MasterTechApp{
    pub fn viewport_loader(&mut self, ctx: &Context) {
        if self.context.show_deferred_viewport.load(Ordering::Relaxed) {
            let file_browser_clone = Arc::clone(&self.context.file_browser);
            let show_deferred_viewport = self.context.show_deferred_viewport.clone();
            let viewport_id = ViewportId::from_hash_of("deferred_viewport");
            let viewport_builder = ViewportBuilder::default().with_title("File Browser").with_inner_size([400.0, 500.0]);

            ctx.show_viewport_deferred(
                viewport_id,
                viewport_builder,
                move |ctx, _class| 
            {
                    CentralPanel::default().show(ctx, |ui| {
                        // Lock the Mutex and show the GUI
                        let mut file_browser = file_browser_clone.lock().unwrap();
                        file_browser.show(ui);
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        // Tell parent to close us.
                        show_deferred_viewport.store(false, Ordering::Relaxed);
                    }
            });
        }

        if self.context.show_ws_viewport.load(Ordering::Relaxed) {
            let show_ws_viewport = self.context.show_ws_viewport.clone();
            let viewport_id = ViewportId::from_hash_of("deferred_viewport_ws_connection");
            let viewport_builder = ViewportBuilder::default()
                .with_title("Websocket Connection")
                .with_inner_size([400.0, 500.0]);

            ctx.show_viewport_immediate(
                viewport_id,
                viewport_builder,
                |ctx, _class| 
            {
                CentralPanel::default().show(ctx, |ui| {
                    if let Some(ref mut frontend) = self.context.frontend {
                        // Lock the Mutex and show the GUI
                        let connected = frontend.initialize_websocket(ui);
                        if !connected{ 
                            if let Some(url) = &self.context.url{
                                std::thread::sleep(Duration::from_secs(10));
                                info!("Trying to reconnect");
                                self.context.make_ws_connection(&url.to_string(), ui.ctx().clone());
                            }
                        }
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    show_ws_viewport.store(false, Ordering::Relaxed); // Tell parent to close us.
                }
            });
        }
    }
}
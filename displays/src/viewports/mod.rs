use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

use eframe::egui::{Context, ViewportBuilder, ViewportId, Window};
use log::info;

use crate::{app_state::SharedContext, modals::{task_modal::ModalAction, ModalType, ModalWindow}};



impl SharedContext {
    pub fn handle_viewports(&mut self, ctx: &Context) {
        for (id, viewport_data) in self.show_tasks_viewport.iter_mut() {
            info!("ID: {id:?}\nviewport: {:?}", viewport_data.is_visible);
            if viewport_data.is_visible.load(Ordering::Relaxed) {
                let viewport_state = viewport_data.is_visible.clone();
                let viewport_id = ViewportId::from_hash_of(id.key().to_string());
                let viewport_builder = ViewportBuilder::default()
                    .with_title("Task Viewport")
                    .with_inner_size([750.0, 850.0]); // Match modal dimensions

                ctx.show_viewport_immediate(
                    viewport_id,
                    viewport_builder,
                    |ctx, _class| 
                {
                    // Render the TaskModal UI
                    let action = viewport_data.modal.ui(ctx, "Test".to_string(), 750., 850.);
                    if let Some(ModalAction::Close) = action {
                        viewport_state.store(false, Ordering::Relaxed); // Close viewport
                    }
                    if ctx.input(|i| i.viewport().close_requested()) {
                        viewport_state.store(false, Ordering::Relaxed); // Handle viewport close
                    }
                });
            }
        }

        let ws_layout = &mut self.web_console_layout;
        let undock_clients = &ws_layout.undock_client;

        for (client_id, wants_to_undock) in undock_clients.iter() {
            info!("ID: {client_id:?}\nviewport: {:?}", wants_to_undock);
            let x = Arc::new(AtomicBool::new(false));
            
            if *wants_to_undock {
                x.store(true, Ordering::Relaxed);
            }

            if x.load(Ordering::Relaxed) {
                let viewport_id = ViewportId::from_hash_of(client_id);
                let viewport_builder = ViewportBuilder::default()
                    .with_title("Task Viewport")
                    .with_inner_size([750.0, 850.0]); // Match modal dimensions

                ctx.show_viewport_immediate(
                    viewport_id,
                    viewport_builder,
                    |ctx, _class| 
                {
                    Window::new(client_id)
                        .auto_sized()
                        .show(ctx, |ui| {
                            if let Some(ws_client) = ws_layout.ws_clients.get_mut(client_id) {
                                ws_client.show(ui);
                            }
                        });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        x.store(false, Ordering::Relaxed); // Handle viewport close
                    }
                });
            }
        }
    }
}


#[derive(Default, Debug)]
pub struct ViewportData {
    pub is_visible: Arc<AtomicBool>,
    pub modal: ModalType,
}
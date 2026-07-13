#![allow(deprecated)]
use crate::{app_state::SharedContext, modals::{task_modal::ModalAction, ModalType, ModalWindow}};
use crate::tabs::admin_console::SessionLayout;
use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportId, Window};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use database::schema::RecordIdExt;
use log::info;

impl SharedContext {
    pub fn handle_viewports(&mut self, ctx: &Context) {
        let ws_layout = &mut self.web_console_layout;

        // Collect the list of Floating sessions to avoid holding an immutable
        // borrow on `session_layout` while we mutably borrow `ws_clients`.
        let floating: Vec<String> = ws_layout
            .session_layout
            .iter()
            .filter(|(_, layout)| **layout == SessionLayout::Floating)
            .map(|(id, _)| id.clone())
            .collect();

        for client_id in floating.iter() {
            if cfg!(not(target_arch = "wasm32")) {
                let viewport_id = ViewportId::from_hash_of(client_id);
                let viewport_builder = ViewportBuilder::default()
                    .with_taskbar(true)
                    .with_min_inner_size([1100., 950.])
                    .with_always_on_top()
                    .with_resizable(true)
                    .with_title(client_id);

                ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                    CentralPanel::default().show(ctx, |ui| {
                        ui.set_min_size([1000., 900.].into());
                        if let Some(ws_client) = ws_layout.ws_clients.get_mut(client_id) {
                            ws_client.show(ui);
                        }
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        // Revert to Docked when the user closes the OS window.
                        ws_layout
                            .session_layout
                            .insert(client_id.clone(), SessionLayout::Docked);
                    }
                });
            } else {
                Window::new(client_id.as_str())
                    .min_size([1100., 950.])
                    .show(ctx, |ui| {
                        CentralPanel::default().show_inside(ui, |ui| {
                            ui.set_min_size([1100., 950.].into());
                            if let Some(ws_client) = ws_layout.ws_clients.get_mut(client_id) {
                                ws_client.show(ui);
                            }
                        });
                    });
            }
        }
    
        // Remote-desktop popouts: one OS window per client with desktop_popout set.
        #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
        {
            let popped: Vec<String> = ws_layout
                .ws_clients
                .iter()
                .filter(|(_, client)| client.desktop_popout)
                .map(|(id, _)| id.clone())
                .collect();

            for client_id in popped.iter() {
                let viewport_id = ViewportId::from_hash_of(("remote_desktop_popout", client_id.as_str()));
                let viewport_builder = ViewportBuilder::default()
                    .with_title(format!("Remote Desktop — {client_id}"))
                    .with_inner_size([1280., 800.])
                    .with_resizable(true)
                    .with_taskbar(true);

                ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                    CentralPanel::default().show(ctx, |ui| {
                        if let Some(client) = ws_layout.ws_clients.get_mut(client_id) {
                            client.desktop_popout_ui(ui);
                        }
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        if let Some(client) = ws_layout.ws_clients.get_mut(client_id) {
                            client.desktop_popout = false;
                        }
                    }
                });
            }
        }

        for (id, viewport_data) in self.show_tasks_viewport.iter_mut() {
            info!("ID: {id:?}\nviewport: {:?}", viewport_data.is_visible);
            if viewport_data.is_visible.load(Ordering::Relaxed) {
                let viewport_state = viewport_data.is_visible.clone();
                let viewport_id = ViewportId::from_hash_of(id.key_string());
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
    }
}


#[derive(Default, Debug)]
pub struct ViewportData {
    pub is_visible: Arc<AtomicBool>,
    pub modal: ModalType,
}
use crate::{app_state::SharedContext, modals::{task_modal::ModalAction, ModalType, ModalWindow}};
use crate::tabs::admin_console::SessionLayout;
use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportId, Window};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use database::schema::RecordIdExt;
use log::info;

impl SharedContext {
    /// Same row header the docked client list renders, so a detached session
    /// keeps its status dot and action buttons.
    fn floating_session_header(
        ui: &mut eframe::egui::Ui,
        ws_layout: &crate::tabs::admin_console::AdminConsole,
        client: Option<&database::schema::ConnectedClient>,
        client_id: &str,
        reachability: &std::collections::HashMap<
            String,
            crate::ui_data::reachability::ReachabilityStatus,
        >,
    ) {
        use crate::tabs::admin_console::client_interface::TransportKind;
        let Some(client) = client else { return };
        let session = ws_layout.ws_clients.get(client_id);
        let is_ws_connected = session
            .map(|w| {
                if w.transport.kind() != TransportKind::WebSocket {
                    w.is_connected
                } else {
                    w.is_connected && w.last_pong_time.is_some()
                }
            })
            .unwrap_or(false);
        ui.horizontal(|ui| {
            crate::tabs::admin_console::AdminConsole::client_header(
                ui,
                ws_layout.ui_actions_channel.0.clone(),
                client,
                ws_layout.session_layout.clone(),
                ws_layout.focused_client.as_deref(),
                is_ws_connected,
                &ws_layout.fk_health_tx,
                &ws_layout.fk_health_cache,
                ws_layout.security_inventory.get(client_id).map(|v| v.as_slice()),
                reachability.get(client_id),
                session.map(|w| (w.transport.kind(), w.is_connected)),
            );
        });
    }

    pub fn handle_viewports(&mut self, ctx: &Context) {
        // Snapshotted before the mut borrow below so the detached window can
        // render the same header the docked row does.
        let reachability_snapshot = self.reachability_cache.clone();
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
            // Falls back to the session's own copy: a client reached by hash is
            // absent from the scoped list.
            let client = ws_layout
                .clients
                .iter()
                .find(|c| &c.connection_string == client_id)
                .cloned()
                .or_else(|| ws_layout.ws_clients.get(client_id).map(|w| w.client.clone()));
            let title = client
                .as_ref()
                .and_then(|c| c.friendly_name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| client_id.clone());

            if cfg!(not(target_arch = "wasm32")) {
                let viewport_id = ViewportId::from_hash_of(client_id);
                let viewport_builder = ViewportBuilder::default()
                    .with_taskbar(true)
                    .with_min_inner_size([1100., 950.])
                    .with_always_on_top()
                    .with_resizable(true)
                    .with_title(&title);

                ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                    CentralPanel::default().show(ctx, |ui| {
                        ui.set_min_size([1000., 900.].into());
                        Self::floating_session_header(
                            ui,
                            ws_layout,
                            client.as_ref(),
                            client_id,
                            &reachability_snapshot,
                        );
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
                Window::new(title.as_str())
                    .min_size([1100., 950.])
                    .show(ctx, |ui| {
                        CentralPanel::default().show(ui, |ui| {
                            ui.set_min_size([1100., 950.].into());
                            Self::floating_session_header(
                                ui,
                                ws_layout,
                                client.as_ref(),
                                client_id,
                                &reachability_snapshot,
                            );
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
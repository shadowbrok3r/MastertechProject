use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportCommand, ViewportId};
use std::{sync::{atomic::Ordering, Arc}, time::Duration};
use displays::tabs::admin_console::client_interface::TransportKind;
use displays::tabs::admin_console::{AdminConsole, SessionLayout};
use crate::app_state::MasterTechApp;
use log::info;

impl MasterTechApp{
    pub fn viewport_loader(&mut self, ctx: &Context) {
        if self.context.show_deferred_viewport.load(Ordering::Relaxed) {
            let file_browser_clone = Arc::clone(&self.context.file_browser);
            let show_deferred_viewport = self.context.show_deferred_viewport.clone();
            let viewport_id = ViewportId::from_hash_of("deferred_viewport");
            let viewport_builder = ViewportBuilder::default().with_title("File Browser").with_inner_size([400.0, 500.0]);
            let tx = self.context.copied_items_tx.clone();
            ctx.show_viewport_deferred(
                viewport_id,
                viewport_builder,
                move |ctx, _class| 
            {
                    CentralPanel::default().show(ctx, |ui| {
                        // Lock the Mutex and show the GUI
                        let mut file_browser = file_browser_clone.lock().unwrap();
                        file_browser.show(ui, tx.clone());
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
                        let connected = frontend.initialize_websocket(ui);
                        if !connected {
                            let should_reconnect = self.context.last_reconnect_attempt
                                .map(|t| t.elapsed() >= Duration::from_secs(5))
                                .unwrap_or(true);
                            if should_reconnect {
                                if let Some(url) = &self.context.url {
                                    info!("Trying to reconnect");
                                    self.context.last_reconnect_attempt = Some(std::time::Instant::now());
                                    self.context.make_ws_connection(&url.to_string(), ui.ctx().clone(), self.context.client_uuid.clone());
                                }
                            }
                        }
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    show_ws_viewport.store(false, Ordering::Relaxed); // Tell parent to close us.
                }
            });
        }

        let layout = &mut self.context.shared_ctx.web_console_layout;
        for client in self.context.shared_ctx.clients.clone() {
            let is_floating = layout
                .session_layout
                .get(&client.connection_string)
                .copied()
                .unwrap_or_default()
                == SessionLayout::Floating;

            if !is_floating {
                continue;
            }

            let client_hash = client.connection_string.clone();
            let client_friendly_name = client.friendly_name.clone();
            let viewport_id =
                ViewportId::from_hash_of(format!("deferred_viewport_ws_connection {client_hash}"));
            let viewport_builder = ViewportBuilder::default()
                .with_title(format!(
                    "{}",
                    client_friendly_name.unwrap_or(client_hash.clone())
                ))
                .with_inner_size([400.0, 500.0]);

            ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                #[allow(deprecated)]
                CentralPanel::default().show(ctx, |ui| {
                    let tx = layout.ui_actions_channel.0.clone();

                    let is_ws_connected = layout
                        .ws_clients
                        .get(&client.connection_string)
                        .map(|wsc| {
                            if wsc.transport.kind() == TransportKind::Tcp {
                                wsc.is_connected
                            } else {
                                wsc.is_connected && wsc.last_pong_time.is_some()
                            }
                        })
                        .unwrap_or(false);

                    let inventory = layout
                        .security_inventory
                        .get(&client.connection_string)
                        .map(|v| v.as_slice());
                    // Floating-viewport renderer has no access to
                    // the SharedContext-side reachability cache.
                    // Passing `None` is correct here — the
                    // detached window doesn't need to show the
                    // probe state (operators look at that in the
                    // main admin console row).
                    ui.horizontal(|ui| {
                        AdminConsole::client_header(
                            ui,
                            tx,
                            &client,
                            layout.session_layout.clone(),
                            layout.focused_client.as_deref(),
                            is_ws_connected,
                            inventory,
                            None,
                        );
                    });
                    if let Some(ws_client) = layout.ws_clients.get_mut(&client.connection_string) {
                        ws_client.show(ui);
                    }
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    layout
                        .session_layout
                        .insert(client.connection_string.clone(), SessionLayout::Docked);
                }
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        for client in self.context.shared_ctx.clients.clone() {
            let conn = client.connection_string.clone();
            let pop = self
                .context
                .shared_ctx
                .web_console_layout
                .ws_clients
                .get(&conn)
                .map(|w| w.egui_remote_popout)
                .unwrap_or(false);
            if !pop {
                continue;
            }

            let title = format!(
                "Remote UI — {}",
                client.friendly_name.clone().unwrap_or_else(|| conn.clone())
            );
            let viewport_id = ViewportId::from_hash_of(format!("egui_remote_popout {conn}"));
            let viewport_builder = ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([960.0, 720.0]);

            ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                let layout = &mut self.context.shared_ctx.web_console_layout;
                if let Some(ws) = layout.ws_clients.get_mut(&conn) {
                    ws.egui_viewer.poll_frames();
                    if let Some((rw, rh)) = ws.egui_viewer.remote_canvas_points() {
                        let inner_w = (rw + 32.0).max(320.0);
                        let inner_h = (rh + 72.0).max(240.0);
                        let key = ((inner_w * 2.0) as u32, (inner_h * 2.0) as u32);
                        if ws.egui_remote_popout_inner_sent != Some(key) {
                            ws.egui_remote_popout_inner_sent = Some(key);
                            ctx.send_viewport_cmd(ViewportCommand::InnerSize(
                                eframe::egui::vec2(inner_w, inner_h),
                            ));
                        }
                    }
                }
                CentralPanel::default().show(ctx, |ui| {
                    let layout = &mut self.context.shared_ctx.web_console_layout;
                    if let Some(ws) = layout.ws_clients.get_mut(&conn) {
                        ws.show_egui_remote_viewport_panel(ui, ctx);
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    if let Some(ws) = self
                        .context
                        .shared_ctx
                        .web_console_layout
                        .ws_clients
                        .get_mut(&conn)
                    {
                        ws.egui_remote_popout = false;
                        ws.egui_remote_popout_inner_sent = None;
                    }
                }
            });
        }
    }
}
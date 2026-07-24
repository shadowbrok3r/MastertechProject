use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportCommand, ViewportId};
use std::sync::{atomic::Ordering, Arc};
use displays::tabs::admin_console::client_interface::TransportKind;
use displays::tabs::admin_console::{AdminConsole, SessionLayout};
use crate::app_state::MasterTechApp;

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

        if self.context.show_terminal_viewport.load(Ordering::Relaxed) {
            // EmbeddedTerminal holds Rc/RefCell state and is not Send, so it
            // renders through an immediate viewport rather than a deferred one.
            let viewport_id = ViewportId::from_hash_of("deferred_viewport_terminal");
            let viewport_builder = ViewportBuilder::default()
                .with_title("Terminal")
                .with_inner_size([1000.0, 700.0]);
            let show_terminal_viewport = self.context.show_terminal_viewport.clone();
            let user = self.context.shared_ctx.current_user.clone();
            ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                CentralPanel::default().show(ctx, |ui| {
                    self.context
                        .embedded_terminal
                        .get_or_insert_with(|| {
                            crate::terminal_mode::embedded::EmbeddedTerminal::new(user.clone())
                        })
                        .ui(ui);
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    show_terminal_viewport.store(false, Ordering::Relaxed);
                }
            });
        }

        // The detached "Websocket Connection" viewport was tied to the
        // GUI-side WS-relay (`tabs/websockets/mod.rs`).  That relay is
        // gone — the direct-TCP `tcp_listener` does not need an
        // operator-visible reconnect UI, so this whole viewport block
        // was removed.  See git history for the original implementation.

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
                    //
                    // Same goes for `fk_health_tx`/`fk_health_cache`:
                    // the detached viewport never owns a probe channel.
                    // Pass an empty placeholder pair so the shared
                    // `client_header` signature stays unified with the
                    // primary admin-console caller (which feeds it real
                    // `ws_client.fk_health_tx`/`fk_health_cache`).  The
                    // viewport will render FK health as "unknown"; the
                    // docked row stays the source of truth for it.
                    let (fk_health_tx, _fk_health_rx) =
                        crossbeam::channel::unbounded::<(String, bool, bool)>();
                    let fk_health_cache: std::collections::HashMap<
                        String,
                        (bool, bool),
                    > = std::collections::HashMap::new();
                    let transport = layout
                        .ws_clients
                        .get(&client.connection_string)
                        .map(|w| (w.transport.kind(), w.is_connected));
                    ui.horizontal(|ui| {
                        AdminConsole::client_header(
                            ui,
                            tx,
                            &client,
                            layout.session_layout.clone(),
                            layout.focused_client.as_deref(),
                            is_ws_connected,
                            &fk_health_tx,
                            &fk_health_cache,
                            inventory,
                            None,
                            transport,
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

            ctx.show_viewport_immediate(viewport_id, viewport_builder, |vp_ui, _class| {
                let layout = &mut self.context.shared_ctx.web_console_layout;
                if let Some(ws) = layout.ws_clients.get_mut(&conn) {
                    ws.egui_viewer.poll_frames();
                    if let Some((rw, rh)) = ws.egui_viewer.remote_canvas_points() {
                        let inner_w = (rw + 32.0).max(320.0);
                        let inner_h = (rh + 72.0).max(240.0);
                        let key = ((inner_w * 2.0) as u32, (inner_h * 2.0) as u32);
                        if ws.egui_remote_popout_inner_sent != Some(key) {
                            ws.egui_remote_popout_inner_sent = Some(key);
                            vp_ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(
                                eframe::egui::vec2(inner_w, inner_h),
                            ));
                        }
                    }
                }
                CentralPanel::default().show(vp_ui, |ui| {
                    let vctx = ui.ctx().clone();
                    let layout = &mut self.context.shared_ctx.web_console_layout;
                    if let Some(ws) = layout.ws_clients.get_mut(&conn) {
                        ws.show_egui_remote_viewport_panel(ui, &vctx);
                    }
                });
                if vp_ui.ctx().input(|i| i.viewport().close_requested()) {
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
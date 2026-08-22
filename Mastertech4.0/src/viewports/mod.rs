use eframe::egui::{CentralPanel, Context, ViewportBuilder, ViewportCommand, ViewportId};
use std::sync::{atomic::Ordering, Arc};
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

        if self.context.show_assist_viewport.load(Ordering::Relaxed) {
            // Immediate: AssistProgress owns crossbeam channels and is polled
            // from the same context the tech's tab uses.
            let viewport_id = ViewportId::from_hash_of("assist_progress_viewport");
            let viewport_builder = ViewportBuilder::default()
                .with_title("AI diagnostic")
                .with_inner_size([520.0, 620.0]);
            let show_assist_viewport = self.context.show_assist_viewport.clone();
            ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, _class| {
                CentralPanel::default().show(ctx, |ui| {
                    match self.context.assist_progress.as_mut() {
                        Some(progress) => progress.ui(ui),
                        None => {
                            ui.label("No AI diagnostic is running.");
                        }
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    show_assist_viewport.store(false, Ordering::Relaxed);
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

        // Floating admin sessions render in `SharedContext::handle_viewports`
        // (displays/src/viewports), which runs right after this. A second
        // renderer here opened a duplicate OS window per undocked client.

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
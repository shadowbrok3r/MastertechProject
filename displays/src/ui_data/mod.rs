use database::{live_data::listen_data_filtered, schema::{utilities::{get_notifications, get_qcs, get_store_users, get_tasks_for_store}, RecordIdExt, TaskNotePayload, TaskNoteRead, User}};
use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, ToastStyle};
use crate::{get_toast_receiver, PlatformSpawner, Spawner, ToastMessage};
use std::sync::Arc;

pub mod receive_notes;
pub mod receive_notifications;
pub mod receive_prestashop;
pub mod receive_task;
pub mod receive_ui_action;
pub mod receive_client;
pub mod receive_users;
pub mod receive_read_state;
pub mod admin_notification;
pub mod reachability;
pub mod open_service_apply;
// pub mod receive_database;

impl crate::app_state::SharedContext {
    pub fn load_data(&mut self, ctx: &eframe::egui::Context, user: &User) {
        self.refresh_client_list();
        self.timer = Some(web_time::Instant::now());
        // get all of our channel Senders from crossbeam to get user/store/completed tasks,
        // as well as store users and live task notifications
        let live_tasks_tx = self.live_tasks_tx.clone();
        let notes_tx = self.notes_tx.clone();
        let live_notif_tx = self.live_notification_tx.clone();        
        let live_user_tx = self.live_user_tx.clone();
        let live_clients_tx = self.live_clients_tx.clone();
        self.store_selection = user.get_store().into_store_id() as u64;
        let user = user.clone();
        let name = user.get_name();
        log::info!("Getting Initial data: {}", self.store_selection);

        if self.filesystem.paths.is_empty() {
            self.filesystem.set_user(user.clone());
            let _ = self.filesystem.request_contents("");
        }

        if self.web_console_layout.filesystem.paths.is_empty() {
            self.web_console_layout.filesystem.set_user(user.clone());
            let _ = self.web_console_layout.filesystem.request_contents("");
            // self.web_console_layout.set_filesystem(self.filesystem.clone());
        }

        if self.tasks.is_empty() || self.store_users.is_empty() {
            let initial_tasks_tx = self.initial_tasks_tx.clone();
            let store_users_tx = self.store_users_tx.clone();
            let store = user.get_store();
            let notifs_tx = self.notification_tx.clone();
            PlatformSpawner::spawn(async move {
                let get_store_users = get_store_users(store_users_tx, store).await;
                log::info!("get_store_users: {get_store_users:?}");
            });

            PlatformSpawner::spawn(async move {
                let get_tasks = get_tasks_for_store(initial_tasks_tx, store.as_str().to_string()).await;
                log::info!("get_tasks: {get_tasks:?}");
            });

            PlatformSpawner::spawn(async move {
                let get_qcs = get_qcs().await;
                log::error!("get_qcs: {get_qcs:?}");
            });

            PlatformSpawner::spawn(async move {
                let get_notifications = get_notifications(notifs_tx).await;
                log::info!("get_notifications: {get_notifications:?}");
            });

            let read_state_tx = self.read_state_tx.clone();
            PlatformSpawner::spawn(async move {
                let res = TaskNoteRead::fetch_all_for_user(read_state_tx).await;
                log::info!("fetch_all_for_user (task_note_read): {res:?}");
            });
            
            self.task_layouts
                .iter_mut()
                .filter(|(page, _)| *page == "Completed Tasks" || *page == "Store Tasks")
                .for_each(|(_, layout)| {
                    layout.loading = false;
            });
        }

        // Live-query fan-out used to crash the SurrealDB pod (see
        // `SurrealCrashes.md`): 40-50 users × 5 unfiltered streams =
        // 200-250 concurrent `LIVE SELECT *` subscriptions. Two
        // changes here:
        //
        //   1. `live_queries_active` gates the whole block. If
        //      `load_data` runs again (re-login, reconnect, etc.),
        //      we don't stack a second set of streams on top of the
        //      still-running first set.
        //   2. Every stream is filtered through
        //      `listen_data_filtered` with a `WHERE` clause scoped to
        //      the user's store, and each stream's UUID is shipped
        //      back through `live_query_uuid_tx` so the
        //      `receive_shared_logic` drain can call `KILL` on
        //      shutdown / re-spawn instead of leaking the streams.
        if !self.live_queries_active {
            self.live_queries_active = true;

            // Clone error channel for each live query
            let error_tx_notes = self.live_query_error_tx.clone();
            let error_tx_users = self.live_query_error_tx.clone();
            let error_tx_tasks = self.live_query_error_tx.clone();
            let error_tx_notifs = self.live_query_error_tx.clone();
            let error_tx_clients = self.live_query_error_tx.clone();

            // task_note → joined through task.assignee.store so a note's
            // visibility tracks the task it's attached to. Matches the
            // pattern used by `TaskNotePayload::get_all_notes_in_my_store`.
            PlatformSpawner::spawn(async move {
                let res = listen_data_filtered::<TaskNotePayload>(
                    notes_tx,
                    "LIVE SELECT * FROM task_note WHERE task_id.assignee.store == $auth.store".to_string(),
                    vec![],
                ).await;
                log::info!("listen_task_notes: {res:?}");
                if let Err(e) = res {
                    let _ = error_tx_notes.try_send(e.to_string());
                }
            });

            // user → other users in the same store (admin presence /
            // settings updates). Unfiltered before; now store-scoped.
            PlatformSpawner::spawn(async move {
                let res = listen_data_filtered::<User>(
                    live_user_tx,
                    "LIVE SELECT * FROM user WHERE store == $auth.store".to_string(),
                    vec![],
                ).await;
                log::info!("listen_user: {res:?}");
                if let Err(e) = res {
                    let _ = error_tx_users.try_send(e.to_string());
                }
            });

            // task → assignee on this store. Matches the initial-fetch
            // shape from `get_tasks_for_store` (assignee.store == $store).
            PlatformSpawner::spawn(async move {
                let res = listen_data_filtered::<database::schema::LiveTaskPayload>(
                    live_tasks_tx,
                    "LIVE SELECT * FROM task WHERE assignee.store == $auth.store".to_string(),
                    vec![],
                ).await;
                log::info!("listen_tasks: {res:?}");
                if let Err(e) = res {
                    let _ = error_tx_tasks.try_send(e.to_string());
                }
            });

            // notification → only this user's notifications. Notifications
            // are addressed via the `user` field, so this is the tightest
            // possible scope. Reduces server fan-out from "every
            // notification table change" to "only mine".
            PlatformSpawner::spawn(async move {
                let res = listen_data_filtered::<database::schema::Notification>(
                    live_notif_tx,
                    "LIVE SELECT * FROM notification WHERE user == $auth.id".to_string(),
                    vec![],
                ).await;
                log::info!("listen_notifications: {res:?}");
                if let Err(e) = res {
                    let _ = error_tx_notifs.try_send(e.to_string());
                }
            });

            // connected_client → only this store's clients. Earlier
            // iterations also gated on `connected == true`, but the
            // admin UI needs to see disconnected rows (to show them as
            // offline in the list); only store-scoping is applied here.
            let is_root = user.get_authorization() == database::schema::user::UserAuthorization::Root;
            PlatformSpawner::spawn(async move {
                let query = if is_root {
                    "LIVE SELECT * FROM connected_client WHERE assigned_user.store == $auth.store AND connected == true".to_string()
                } else {
                    "LIVE SELECT * FROM connected_client WHERE assigned_user == $auth.id AND assigned_user.store == $auth.store AND connected == true".to_string()
                };
                let res = listen_data_filtered::<database::schema::ConnectedClient>(
                    live_clients_tx,
                    query,
                    vec![],
                ).await;
                log::info!("listen_connected_clients: {res:?}");
                if let Err(e) = res {
                    let _ = error_tx_clients.try_send(e.to_string());
                }
            });
        } else {
            log::info!("load_data: live queries already active; skipping re-spawn");
        }

        // Slice 5: kick off the per-admin TCP reachability prober.
        // The prober loops forever until `wait_for_shutdown` fires,
        // reading the current client list from the in-memory snapshot
        // updated by `receive_client` (no per-round DB query) and
        // shipping probe results back via `reachability_tx`. The UI
        // drains them in `receive_shared_ui::drain_reachability_events`.
        // Not available in WASM — raw TCP sockets don't exist in browsers.
        #[cfg(not(target_arch = "wasm32"))]
        reachability::spawn_prober(
            self.reachability_tx.clone(),
            self.clients_for_prober.clone(),
        );

        self.stock_tables.first_run();
        crate::ui_tools::theme_config::apply_user_color_scheme(ctx, &user.get_color_scheme());
        self.user_theme_loaded = true;
        ctx.request_repaint();

        let new_notes_tx = self.associated_notes_tx.clone();
        
        PlatformSpawner::spawn(async move {
            let get_notes = TaskNotePayload::get_all_notes_in_my_store(new_notes_tx).await;
            log::info!("get_notes: {get_notes:?}");
        });

        ctx.request_repaint();
        
        let toast = &mut self.toasts;
        let auth_toast = Toast {
            kind: ToastKind::Success,
            text: format!("Logged in successfully\nWelcome, {}", name).into(),
            options: ToastOptions::default()
                .show_progress(true)
                .duration_in_seconds(6.0),
            style: ToastStyle::default(),
        };
        toast.add(auth_toast);
    }

    pub fn receive_shared(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        self.receive_shared_logic(frame, ctx);
        self.receive_shared_ui(ctx);
    }

    /// All channel polling and state mutations -- no UI rendering.
    /// Called from `fn logic` so it runs even when the window is hidden.
    pub fn receive_shared_logic(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        if !self.user_theme_loaded {
            crate::ui_tools::theme_config::bootstrap_startup_theme(ctx);
        }

        ctx.request_repaint_after(web_time::Duration::from_secs(1));
        if let Ok(error_msg) = self.live_query_error_rx.try_recv() {
            log::warn!("Live query connection error detected: {}", error_msg);
            let looks_transient = error_msg.contains("connection reset")
                || error_msg.contains("reset")
                || error_msg.contains("I/O")
                || error_msg.contains("ConnectionFailed")
                || error_msg.contains("stream terminated");
            if looks_transient {
                // Cooldown swallows the burst of 5 identical errors from one blip.
                const RESPAWN_COOLDOWN: web_time::Duration =
                    web_time::Duration::from_secs(10);
                // Quiet period after which the auto-reconnect budget refills.
                const ATTEMPT_RESET_WINDOW: web_time::Duration =
                    web_time::Duration::from_secs(60);
                const MAX_AUTO_RECONNECT_ATTEMPTS: u32 = 2;
                let now = web_time::Instant::now();
                let in_cooldown = self
                    .last_live_respawn_at
                    .map(|t| now.duration_since(t) < RESPAWN_COOLDOWN)
                    .unwrap_or(false);
                if in_cooldown {
                    log::debug!(
                        "Live query banner skipped — within cooldown window \
                         (last fired {:?} ago)",
                        now.duration_since(self.last_live_respawn_at.unwrap())
                    );
                } else {
                    if self
                        .last_live_respawn_at
                        .map(|t| now.duration_since(t) >= ATTEMPT_RESET_WINDOW)
                        .unwrap_or(false)
                    {
                        self.reconnect_attempts = 0;
                    }
                    self.live_queries_active = false;
                    self.last_live_respawn_at = Some(now);

                    let user = self.current_user.clone();
                    if self.reconnect_attempts < MAX_AUTO_RECONNECT_ATTEMPTS
                        && user.is_some()
                    {
                        self.reconnect_attempts += 1;
                        log::warn!(
                            "Stream-terminated error — auto-reconnect attempt {} of {}",
                            self.reconnect_attempts,
                            MAX_AUTO_RECONNECT_ATTEMPTS
                        );
                        self.needs_reconnect = true;
                        let user = user.unwrap();
                        self.load_data(ctx, &user);
                    } else {
                        log::warn!(
                            "Auto-reconnect exhausted (attempts={}) — prompting operator",
                            self.reconnect_attempts
                        );
                        self.needs_reconnect = true;
                        self.show_reload_prompt = true;
                    }
                }
            }
        }

        // `document.visibilitychange` is no longer treated as evidence of
        // a broken connection — the SurrealDB WS survives short hides and
        // the Cloudflare Tunnel idle window is long. Instead: stamp when
        // the tab goes hidden, and on return-to-foreground only force a
        // hard reload when the hide duration exceeded `LONG_HIDE_AUTO_RELOAD`
        // (browser tab-suspend territory — the JS runtime was almost
        // certainly paused). Sub-threshold hides clear the stamp and do
        // nothing; the live-query error path is the source of truth for
        // "the connection actually broke."
        const LONG_HIDE_AUTO_RELOAD: web_time::Duration =
            web_time::Duration::from_secs(60 * 45);
        while let Ok(is_visible) = self.visibility_signal_rx.try_recv() {
            if is_visible {
                if let Some(hidden_at) = self.tab_hidden_at.take() {
                    let elapsed = web_time::Instant::now().duration_since(hidden_at);
                    log::info!("Tab visible after {:?} hidden", elapsed);
                    if elapsed >= LONG_HIDE_AUTO_RELOAD {
                        log::warn!(
                            "Tab was hidden for {:?} (>= {:?}) — auto-reloading page",
                            elapsed,
                            LONG_HIDE_AUTO_RELOAD
                        );
                        #[cfg(target_arch = "wasm32")]
                        {
                            if let Some(win) = web_sys::window() {
                                let _ = win.location().reload();
                            }
                        }
                    }
                }
            } else {
                log::info!("Tab hidden");
                self.tab_hidden_at = Some(web_time::Instant::now());
            }
        }

        if let Ok(state) = self.app_state_rx.try_recv() {
            log::info!("Got a new state: {state:?}\nbefore state: {:?}", self.state);
            if let crate::app_state::AppState::NoAuth(reason) = &state {
                let toast = &mut self.toasts;
                let error_toast = crate::ui_tools::toasts::Toast {
                    kind: crate::ui_tools::toasts::ToastKind::Error,
                    text: reason.into(),
                    options: crate::ui_tools::toasts::ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                    style: ToastStyle::default(),
                };
                toast.add(error_toast);
            }

            self.state = state;
            ctx.request_repaint();
        }

        self.koth.receive();
        self.query_editor.receive();
        self.receive_ui_action();
        self.receive_read_state();
        self.receive_users();
        self.receive_task();
        self.receive_notes();
        self.receive_notification(ctx);
        self.stock_tables.receive(self.ui_actions_tx.clone());
        self.sales_tracker.receive();
        self.receive_client();
        self.receive_prestashop();
        self.receive_extracted_specs();
        self.filesystem.receive();
        
        // Deduplicate back-to-back identical toasts within a short
        // window. Without this, a repeating signal like the
        // admin_transport reconnect loop (one toast every 3 s) buries
        // the toast stack and the user has to dismiss the same message
        // dozens of times. The window is intentionally short — a real
        // recurring problem will surface again once the previous toast
        // has had time to be read.
        const DEDUP_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);
        let toast_rx = get_toast_receiver();
        while let Ok(msg) = toast_rx.try_recv() {
            let (kind, text) = match msg {
                ToastMessage::Success(text) => (ToastKind::Success, text),
                ToastMessage::Error(text) => (ToastKind::Error, text),
                ToastMessage::Warning(text) => (ToastKind::Warning, text),
                ToastMessage::Info(text) => (ToastKind::Info, text),
            };

            let now = web_time::Instant::now();
            let is_dup = self
                .last_toast
                .as_ref()
                .is_some_and(|(prev_text, ts)| prev_text == &text && now.duration_since(*ts) < DEDUP_WINDOW);
            if is_dup {
                // Skip — but still bump the timestamp so a burst of
                // identical retries gets fully collapsed rather than
                // re-firing every DEDUP_WINDOW.
                if let Some(entry) = self.last_toast.as_mut() {
                    entry.1 = now;
                }
                continue;
            }
            self.last_toast = Some((text.clone(), now));

            self.toasts.add(Toast {
                kind,
                text: text.into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(3.0),
                style: ToastStyle::default(),
            });
        }
        
        for (_, layout) in self.task_layouts.iter_mut() {
            layout.receive();
        }
        
        self.task_audit_table.receive(self.store_users.clone(), frame);

        if let Ok(releases) = self.github_releases_channel.1.try_recv() {
            log::debug!("Releases: {releases:?}");
            ctx.request_repaint();
            self.github_releases = releases;
        }

        if let Ok(settings) = self.settings_receiver.try_recv() {
            ctx.request_repaint();
            ctx.set_global_style(Arc::new(settings));
        }

        if let Ok(thread_obj) = self.ai_thread_channel.1.try_recv() {
            let mut thread_map = std::collections::HashMap::new();
            self.ai_playground.save_chats = true;
            thread_map.insert(thread_obj.id.clone(), crate::tabs::ai_playground::ChatThread {
                id: thread_obj.id.clone(),
                messages: Vec::new(),
                images: Vec::new(),
                input: String::new(),
            });
            self.ai_playground.selected_thread = thread_obj.id;
            self.ai_playground.set_threads(thread_map);
        }
    }

    /// UI rendering only -- toasts, modals, viewports, admin notifications.
    /// Called from `fn ui` where widget creation is allowed.
    pub fn receive_shared_ui(&mut self, ctx: &eframe::egui::Context) {
        self.admin_notification_ui(ctx);
        self.handle_viewports(ctx);
        self.handle_modals(ctx);
        self.client_diagnostics_popup_ui(ctx);
        self.drain_reachability_events();
        #[cfg(target_arch = "wasm32")]
        self.reload_prompt_ui(ctx);
        self.toasts.show(ctx);
    }

    /// Action-required banner shown when `live_query_error_rx` reports
    /// a stream-terminated error. There is no automatic reconnect path
    /// anymore — the operator clicks "Reconnect" to call `load_data`
    /// directly (the same pattern Mastertech4.0 uses), which re-issues
    /// the five LIVE SELECT subscriptions against whatever WS the
    /// SurrealDB SDK is currently holding. If that fails too, the
    /// operator can click "Reload page" to fully restart the WASM app.
    fn reload_prompt_ui(&mut self, ctx: &eframe::egui::Context) {
        if !self.show_reload_prompt {
            return;
        }
        let mut reload_clicked = false;
        let mut reconnect_clicked = false;
        eframe::egui::Window::new("Connection lost")
            .anchor(eframe::egui::Align2::CENTER_TOP, [0.0, 60.0])
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(
                eframe::egui::Frame::new()
                    .fill(eframe::egui::Color32::from_rgb(60, 30, 30))
                    .stroke(eframe::egui::Stroke::new(
                        1.5,
                        eframe::egui::Color32::from_rgb(220, 80, 80),
                    )),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        eframe::egui::RichText::new("Real-time updates are offline")
                            .strong()
                            .size(14.0)
                            .color(eframe::egui::Color32::from_rgb(255, 200, 200)),
                    );
                    ui.label(
                        eframe::egui::RichText::new(
                            "The live subscription was dropped. Try reconnecting first, \
                            or reload the page if that doesn't recover.",
                        )
                        .color(eframe::egui::Color32::from_rgb(230, 200, 200)),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Reconnect").clicked() {
                            reconnect_clicked = true;
                        }
                        if ui.button("Reload page").clicked() {
                            reload_clicked = true;
                        }
                    });
                    ui.add_space(4.0);
                });
            });
        if reconnect_clicked {
            log::info!(
                "Reconnect button clicked — calling load_data to re-issue LIVE SELECTs"
            );
            self.show_reload_prompt = false;
            self.reconnect_attempts = 0;
            // Snapshot current_user before calling load_data so we don't
            // hold an aliasing borrow of self.
            let user = self.current_user.clone();
            if let Some(user) = user {
                self.load_data(ctx, &user);
            } else {
                log::warn!("Reconnect clicked but current_user is None");
            }
        }
        if reload_clicked {
            #[cfg(target_arch = "wasm32")]
            {
                if let Some(win) = web_sys::window() {
                    let _ = win.location().reload();
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                // Native: reload doesn't apply. Just dismiss; the
                // operator can hit Reconnect again if they want to retry.
                self.show_reload_prompt = false;
            }
        }
    }

    /// Slice 5: drain the background prober's results into the
    /// per-admin `reachability_cache`. Cheap — typically zero or
    /// a handful of events per frame, only the round-completion
    /// burst sees more.
    fn drain_reachability_events(&mut self) {
        while let Ok(event) = self.reachability_rx.try_recv() {
            self.reachability_cache.insert(event.connection_string, event.status);
        }
    }

    /// Per-frame pump + renderer for the connected-client diagnostics
    /// popup (the popup the "🔬 Diagnostics" button on a My Tasks card
    /// triggers).
    ///
    /// Flow:
    ///   1. Drain any `DiagnosticSessionView`s posted by the
    ///      background loader.
    ///   2. If the popup target changed since we last fetched, clear
    ///      state and spawn a fresh `list_for_connection` query.
    ///   3. If the popup is open, render an `egui::Window` that reuses
    ///      the same `display_diagnostics_page` widget the Task Modal
    ///      already uses — so the user sees identical layout
    ///      regardless of where they opened diagnostics from.
    fn client_diagnostics_popup_ui(&mut self, ctx: &eframe::egui::Context) {
        // 1. Drain incoming sessions.
        while let Ok(view) = self.client_diagnostics_rx.try_recv() {
            self.client_diagnostics_sessions.push(view);
        }

        // Take a copy of the target so we can mutate state below without
        // borrow conflicts.
        let Some(target) = self.client_diagnostics_popup.clone() else {
            return;
        };

        // 2. If the popup target changed, kick off a refetch.
        let needs_load = match &self.client_diagnostics_loaded_for {
            Some(prev) if prev == &target => false,
            _ => true,
        };
        if needs_load {
            self.client_diagnostics_sessions.clear();
            self.client_diagnostics_error = None;
            self.client_diagnostics_selected = None;
            self.client_diagnostics_loading = true;
            self.client_diagnostics_loaded_for = Some(target.clone());

            let tx = self.client_diagnostics_tx.clone();
            let cs = target.clone();
            crate::PlatformSpawner::spawn(async move {
                match database::schema::DiagnosticSession::list_for_connection(&cs).await {
                    Ok(sessions) => {
                        for s in sessions {
                            // Re-fetch the full session each time to get
                            // the entries — `list_for_connection`
                            // returns the bare session rows.
                            let entries = match database::schema::DiagnosticSession::get_full(
                                &s.id.key_string(),
                            )
                            .await
                            {
                                Ok(Some(full)) => full.entries,
                                _ => Vec::new(),
                            };
                            let _ = tx.try_send(crate::modals::tabs::DiagnosticSessionView {
                                session: s,
                                entries,
                            });
                        }
                    }
                    Err(e) => {
                        log::error!("client_diagnostics_popup: load failed for {cs}: {e:?}");
                    }
                }
            });
        }

        // 3. Render the popup. Mark loading complete once the channel
        // has at least drained once and is empty — there's no explicit
        // "done" signal so we treat "no new items pending" as done. A
        // future iteration could send a sentinel.
        if self.client_diagnostics_loading
            && self.client_diagnostics_rx.is_empty()
            && !self.client_diagnostics_sessions.is_empty()
        {
            self.client_diagnostics_loading = false;
        }

        let mut still_open = true;
        eframe::egui::Window::new(format!("Diagnostics — {target}"))
            .id(eframe::egui::Id::new(("client_diagnostics_popup", &target)))
            .open(&mut still_open)
            .resizable(true)
            .collapsible(false)
            .default_size([720.0, 560.0])
            .min_width(480.0)
            .show(ctx, |ui| {
                let avail = ui.available_size();
                crate::modals::tabs::display_diagnostics_page(
                    ui,
                    avail,
                    &self.client_diagnostics_sessions,
                    self.client_diagnostics_loading,
                    self.client_diagnostics_error.as_deref(),
                    &mut self.client_diagnostics_selected,
                    // The Admin Console popup has no associated ticket,
                    // so check-in notes are empty here — the widget
                    // already renders a placeholder when they are.
                    "",
                );
            });

        if !still_open {
            // User closed the window; reset everything so reopening
            // refetches cleanly.
            self.client_diagnostics_popup = None;
            self.client_diagnostics_loaded_for = None;
            self.client_diagnostics_sessions.clear();
            self.client_diagnostics_loading = false;
            self.client_diagnostics_error = None;
            self.client_diagnostics_selected = None;
        }
    }
}


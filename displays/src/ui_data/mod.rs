use database::{live_data::listen_data,schema::{utilities::{get_notifications, get_qcs, get_store_users, get_tasks_for_store}, RecordIdExt, TaskNotePayload, TaskNoteRead, User, CONNECTED_CLIENT_TABLE, NOTIFICATION_TABLE, TASK_NOTE_TABLE, TASK_TABLE, USER_TABLE}};
use crate::ui_tools::{decode_style, toasts::{Toast, ToastKind, ToastOptions, ToastStyle}};
use crate::{get_toast_receiver, PlatformSpawner, Spawner, ToastMessage};
use eframe::egui::Style;
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

        // Clone error channel for each live query
        let error_tx_notes = self.live_query_error_tx.clone();
        let error_tx_users = self.live_query_error_tx.clone();
        let error_tx_tasks = self.live_query_error_tx.clone();
        let error_tx_notifs = self.live_query_error_tx.clone();
        let error_tx_clients = self.live_query_error_tx.clone();
        
        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(notes_tx, TASK_NOTE_TABLE).await;
            log::info!("listen_task_notes: {listen_data:?}");
            if let Err(e) = listen_data {
                let error_msg = e.to_string();
                log::error!("Live query error (notes): {}", error_msg);
                let _ = error_tx_notes.try_send(error_msg);
            }
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_user_tx, USER_TABLE).await;
            log::info!("listen_user: {listen_data:?}");
            if let Err(e) = listen_data {
                let error_msg = e.to_string();
                log::error!("Live query error (users): {}", error_msg);
                let _ = error_tx_users.try_send(error_msg);
            }
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_tasks_tx, TASK_TABLE).await;
            log::info!("listen_tasks: {listen_data:?}");
            if let Err(e) = listen_data {
                let error_msg = e.to_string();
                log::error!("Live query error (tasks): {}", error_msg);
                let _ = error_tx_tasks.try_send(error_msg);
            }
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_notif_tx.clone(), NOTIFICATION_TABLE).await;
            log::info!("listen_notifications: {listen_data:?}");
            if let Err(e) = listen_data {
                let error_msg = e.to_string();
                log::error!("Live query error (notifications): {}", error_msg);
                let _ = error_tx_notifs.try_send(error_msg);
            }
        });

        PlatformSpawner::spawn(async move {
            let listen_data = listen_data(live_clients_tx, CONNECTED_CLIENT_TABLE).await;
            log::info!("listen_connected_clients: {listen_data:?}");
            if let Err(e) = listen_data {
                let error_msg = e.to_string();
                log::error!("Live query error (connected_clients): {}", error_msg);
                let _ = error_tx_clients.try_send(error_msg);
            }
        });

        self.stock_tables.first_run();
        match decode_style(&user.get_color_scheme()) {
            Ok(color_settings) => {
                ctx.set_style(color_settings);
                ctx.request_repaint();
            },
            Err(e) => {
                log::error!("Error setting theme config: {e:?}");
                match serde_json::from_str::<Style>(crate::STYLE) {
                    Ok(theme) => {
                        let style = Arc::new(theme);
                        ctx.set_style(style);
                    }
                    Err(e) => log::error!("Error setting theme: {e:?}")
                };
            },
        }

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

    /// Backward-compatible method that calls both logic and UI halves.
    /// Used by MtechServer2.0 which hasn't split fn logic / fn ui yet.
    pub fn receive_shared(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        self.receive_shared_logic(frame, ctx);
        self.receive_shared_ui(ctx);
    }

    /// All channel polling and state mutations -- no UI rendering.
    /// Called from `fn logic` so it runs even when the window is hidden.
    pub fn receive_shared_logic(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        ctx.request_repaint_after(web_time::Duration::from_secs(1));
        
        #[cfg(target_arch = "wasm32")]
        if let Ok(error_msg) = self.live_query_error_rx.try_recv() {
            log::warn!("Live query connection error detected: {}", error_msg);
            if error_msg.contains("connection reset") || error_msg.contains("reset") || error_msg.contains("I/O") {
                log::warn!("Connection reset detected - setting needs_reconnect flag");
                self.needs_reconnect = true;
                self.toasts.add(Toast {
                    kind: ToastKind::Warning,
                    text: "Connection lost. Reconnecting...".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(4.0),
                    style: ToastStyle::default(),
                });
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
        self.receive_notification();
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
            ctx.set_style(settings);
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
        self.toasts.show(ctx);
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


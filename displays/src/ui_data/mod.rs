use database::{live_data::{listen_data_filtered, Action}, schema::{utilities::{get_notifications, get_qcs, get_store_users, get_tasks_for_store}, RecordIdExt, TaskNotePayload, TaskNoteRead, User}};
use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, ToastStyle};
use crate::{get_toast_receiver, PlatformSpawner, Spawner, ToastMessage};
use crate::app_state::ReconnectOutcome;
use crossbeam::channel::Sender;
use std::sync::Arc;

pub mod receive_notes;
pub mod receive_notifications;
pub mod receive_ai_task;
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

/// Which connected clients the admin console subscribes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ClientScope {
    /// Clients assigned to the signed-in user. The only scope non-root gets.
    #[default]
    MyClients,
    /// Every client assigned to a user in the signed-in user's store.
    MyStore,
    /// Every connected client in the fleet, all stores. Root only.
    AllClients,
}

impl ClientScope {
    pub fn label(self) -> &'static str {
        match self {
            ClientScope::MyClients => "My clients",
            ClientScope::MyStore => "My store",
            ClientScope::AllClients => "All clients",
        }
    }

    /// Scopes the given user may select; non-root is limited to their own.
    pub fn selectable_for(user_is_root: bool) -> &'static [ClientScope] {
        if user_is_root {
            &[
                ClientScope::MyClients,
                ClientScope::MyStore,
                ClientScope::AllClients,
            ]
        } else {
            &[ClientScope::MyClients]
        }
    }
}

/// Builds the `connected_client` LIVE query for `scope`, clamping non-root
/// users to [`ClientScope::MyClients`] regardless of what is stored or
/// requested — this is the authoritative gate, not the combo box.
///
/// Every scope keeps `connected == true` so the subscription tracks online
/// machines rather than the whole table.
pub fn connected_client_live_query(scope: ClientScope, user: &User) -> String {
    let is_root =
        user.get_authorization() == database::schema::user::UserAuthorization::Root;
    let effective = if is_root { scope } else { ClientScope::MyClients };
    if effective != scope {
        log::warn!(
            "client scope {:?} requires Root; using {:?}",
            scope,
            effective
        );
    }
    match effective {
        ClientScope::MyClients => "LIVE SELECT * FROM connected_client WHERE \
             assigned_user == $auth.id AND assigned_user.store == $auth.store \
             AND connected == true"
            .to_string(),
        // Root also receives pre-boot UEFI/QC agents (no assigned_user).
        ClientScope::MyStore => "LIVE SELECT * FROM connected_client WHERE \
             (assigned_user.store == $auth.store AND connected == true) \
             OR (client_kind IN ['qc_agent', 'uefi'] AND connected == true)"
            .to_string(),
        ClientScope::AllClients => {
            "LIVE SELECT * FROM connected_client WHERE connected == true".to_string()
        }
    }
}

impl crate::app_state::SharedContext {
    /// Spawns one abortable live-query stream; stream failures report
    /// `(epoch, error)` so stale generations can be discarded.
    fn spawn_live_stream<T>(&mut self, tx: Sender<(Action, T)>, query: String)
    where
        T: serde::de::DeserializeOwned
            + serde::Serialize
            + std::fmt::Debug
            + std::marker::Unpin
            + database::SurrealValue
            + Send
            + 'static,
    {
        let epoch = self.live_epoch;
        let error_tx = self.live_query_error_tx.clone();
        let registered = self.live_registered.clone();
        self.live_streams_expected += 1;
        let (fut, handle) = futures::future::abortable(async move {
            let res = listen_data_filtered::<T>(tx, query.clone(), vec![], Some(registered)).await;
            log::info!("live stream ended (`{query}`): {res:?}");
            if let Err(e) = res {
                let _ = error_tx.try_send((epoch, e.to_string()));
            }
        });
        self.live_stream_aborts.push(handle);
        PlatformSpawner::spawn(async move {
            let _ = fut.await;
        });
    }

    /// Closes every open admin↔client session and drops the client lists.
    ///
    /// Called whenever the signed-in identity goes away: sessions and their
    /// background retry loops outlive a `$auth` change, so without this the
    /// next user inherits the previous user's sessions — including any the
    /// previous (Root) user opened outside the new user's scope — and can act
    /// on them through paths that read `ws_clients` directly (Batch menu).
    pub fn end_admin_sessions(&mut self, reason: &str) {
        let layout = &mut self.web_console_layout;
        if !layout.ws_clients.is_empty() {
            log::info!(
                "Closing {} admin session(s): {reason}",
                layout.ws_clients.len()
            );
        }
        for (cs, mut ws) in layout.ws_clients.drain() {
            log::info!("end_admin_sessions -> closing {cs}");
            ws.transport.close();
        }
        layout.session_layout.clear();
        layout.focused_client = None;
        layout.clients.clear();
        layout.manual_connect_input.clear();
        layout.manual_connect_status.clear();
        layout.manual_connect_busy = false;
        self.clients.clear();
    }

    /// Aborts all live-query streams (dropping a stream auto-KILLs it) and
    /// advances the epoch so late errors from them are ignored.
    pub fn kill_live_streams(&mut self) {
        for handle in self.live_stream_aborts.drain(..) {
            handle.abort();
        }
        self.live_epoch = self.live_epoch.wrapping_add(1);
        self.live_queries_active = false;
        self.chat_streams_active = false;
        // Fresh counter per generation so an aborted stream's late increment
        // can never inflate the next generation's registration count.
        self.live_registered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.live_streams_expected = 0;
        self.live_spawned_at = None;
    }

    /// Spawns the chat live streams (participant-filtered) once the chat tab
    /// has requested them; re-run after each reconnect generation.
    fn spawn_chat_streams(&mut self) {
        let (thread_tx, msg_tx) = self.user_chat.live_stream_senders();
        self.spawn_live_stream::<database::schema::ChatThread>(
            thread_tx,
            "LIVE SELECT * FROM chat_thread WHERE thread_users CONTAINS $auth.id".to_string(),
        );
        self.spawn_live_stream::<database::schema::UserMessage>(
            msg_tx,
            "LIVE SELECT * FROM user_message WHERE thread_id.thread_users CONTAINS $auth.id".to_string(),
        );
        self.chat_streams_active = true;
    }

    pub fn load_data(&mut self, ctx: &eframe::egui::Context, user: &User) {
        let first_load = self.timer.is_none();
        // A fresh login starts the reconnect supervisor from a clean slate so
        // stale backoff/error state from a prior session can't carry over.
        if first_load {
            self.reconnect_attempts = 0;
            self.needs_reconnect = false;
            self.last_stream_error_at = None;
            self.last_live_respawn_at = None;
            self.last_force_refetch_at = None;
            // Bucket definition runs off the login hot path; covers both
            // fresh-signin and JWT-cookie-restore sessions.
            let bucket_user = user.clone();
            PlatformSpawner::spawn(async move {
                if let Err(e) = database::ensure_user_bucket(&bucket_user).await {
                    log::warn!("ensure_user_bucket failed (retried on File Browser use): {e:?}");
                }
            });
        }
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

        // Bucket listings are lazy: the File Browser / web-console explorer
        // request contents on first render instead of during login.
        if self.filesystem.paths.is_empty() {
            self.filesystem.set_user(user.clone());
        }

        if self.web_console_layout.filesystem.paths.is_empty() {
            self.web_console_layout.filesystem.set_user(user.clone());
        }

        // A reconnect sets `force_data_refetch`: live events missed during
        // the outage are never replayed, so the snapshot must be re-pulled.
        let force_refetch = std::mem::take(&mut self.force_data_refetch);
        if self.tasks.is_empty() || self.store_users.is_empty() || force_refetch {
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

            // AI task snapshot — mandatory on refetch: live events missed
            // during an outage are never replayed.
            let initial_ai_tasks_tx = self.initial_ai_tasks_tx.clone();
            PlatformSpawner::spawn(async move {
                match database::schema::AiTask::list_active_for_store().await {
                    Ok(pair) => { let _ = initial_ai_tasks_tx.try_send(pair); }
                    Err(e) => log::error!("AiTask::list_active_for_store: {e:?}"),
                }
            });
            
            self.task_layouts
                .iter_mut()
                .filter(|(page, _)| *page == "Completed Tasks" || *page == "Store Tasks")
                .for_each(|(_, layout)| {
                    layout.loading = false;
            });
        }

        // Every stream is store-scoped (unfiltered fan-out used to crash the
        // SurrealDB pod, see `SurrealCrashes.md`), spawned abortable, and
        // epoch-tagged. Reconnects call `kill_live_streams()` first so a
        // second set of subscriptions is never stacked on the first.
        if !self.live_queries_active {
            self.live_queries_active = true;
            self.live_registered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            self.live_streams_expected = 0;
            self.live_spawned_at = Some(web_time::Instant::now());

            // task_note → joined through task.assignee.store so a note's
            // visibility tracks the task it's attached to.
            self.spawn_live_stream::<TaskNotePayload>(
                notes_tx,
                "LIVE SELECT * FROM task_note WHERE task_id.assignee.store == $auth.store".to_string(),
            );

            // user → other users in the same store.
            self.spawn_live_stream::<User>(
                live_user_tx,
                "LIVE SELECT * FROM user WHERE store == $auth.store".to_string(),
            );

            // task → assignees on this store.
            self.spawn_live_stream::<database::schema::LiveTaskPayload>(
                live_tasks_tx,
                "LIVE SELECT * FROM task WHERE assignee.store == $auth.store".to_string(),
            );

            // notification → only this user's notifications.
            self.spawn_live_stream::<database::schema::Notification>(
                live_notif_tx,
                "LIVE SELECT * FROM notification WHERE user == $auth.id".to_string(),
            );

            // ai_task → this store's AI handoff tasks.
            self.spawn_live_stream::<database::schema::AiTask>(
                self.live_ai_tasks_tx.clone(),
                "LIVE SELECT * FROM ai_task WHERE assignee.store == $auth.store".to_string(),
            );

            // ai_task_item → joined through the parent's assignee store.
            self.spawn_live_stream::<database::schema::AiTaskItem>(
                self.live_ai_task_items_tx.clone(),
                "LIVE SELECT * FROM ai_task_item WHERE ai_task_ref.assignee.store == $auth.store".to_string(),
            );

            // connected_client → scope selected in the admin console.
            let query = connected_client_live_query(self.client_scope, &user);
            self.spawn_live_stream::<database::schema::ConnectedClient>(live_clients_tx, query);
        } else {
            log::info!("load_data: live queries already active; skipping re-spawn");
        }

        // Per-admin TCP reachability prober; spawned once — the loop runs
        // for the app's lifetime, so reconnect-driven `load_data` calls must
        // not stack additional probers. Not available in WASM.
        #[cfg(not(target_arch = "wasm32"))]
        if first_load {
            reachability::spawn_prober(
                self.reachability_tx.clone(),
                self.clients_for_prober.clone(),
            );
        }

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

        // Reconnect-driven reloads stay silent; recovery is announced by the
        // canary confirmation instead.
        if first_load {
            let toast = &mut self.toasts;
            let auth_toast = Toast {
                kind: ToastKind::Success,
                text: format!("Logged in successfully\nWelcome, {}", name).into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(6.0),
                style: ToastStyle::default(),
                ..Default::default()
            };
            toast.add(auth_toast);
        }
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

        // Scope change: re-issue the live queries under the new filter. The old
        // client set belongs to the previous scope, so drop it and refetch.
        if self.client_scope_dirty {
            self.client_scope_dirty = false;
            if let Some(user) = self.current_user.clone() {
                log::info!("client scope -> {:?}; re-issuing live queries", self.client_scope);
                self.clients.clear();
                self.web_console_layout.clients.clear();
                self.kill_live_streams();
                self.force_data_refetch = true;
                self.load_data(ctx, &user);
            }
        }

        // Drain the full error backlog every frame (five streams die at once
        // on a WS reset); errors from replaced stream generations are noise.
        // Stamp the most recent error so the canary knows not to refill the
        // backoff budget while a stream is still actively failing.
        let mut reconnect_wanted = false;
        while let Ok((epoch, error_msg)) = self.live_query_error_rx.try_recv() {
            if epoch != self.live_epoch {
                log::debug!("Ignoring stale live-query error (epoch {epoch}): {error_msg}");
                continue;
            }
            log::warn!("Live query connection error detected: {}", error_msg);
            self.last_stream_error_at = Some(web_time::Instant::now());
            reconnect_wanted = true;
        }
        if reconnect_wanted {
            // A permanently-failing stream can't storm: the backoff (below)
            // caps at 60s and the canary won't refill it while errors keep
            // arriving, so this self-throttles to one retry per 60s and
            // self-heals if the stream recovers.
            self.trigger_live_reconnect(ctx);
        }

        // Reconnect finished: kill the old streams, re-issue the LIVE SELECTs,
        // refetch the snapshot the gap may have dropped, and force a canary
        // round-trip to confirm delivery. A genuine socket recovery refills
        // the backoff budget; a live-but-single-stream re-issue does not.
        // Results from attempts the stall watchdog abandoned are discarded.
        while let Ok((token, outcome)) = self.reconnect_result_rx.try_recv() {
            if token != self.reconnect_token {
                log::debug!("Ignoring result from abandoned reconnect attempt {token}: {outcome:?}");
                continue;
            }
            self.reconnect_in_progress = false;
            self.reconnect_started_at = None;
            self.reconnect_rebuilding = false;
            match outcome {
                ReconnectOutcome::Ok { socket_was_down, rebuilt } => {
                    log::info!("Reconnect OK (socket_was_down={socket_was_down}, rebuilt={rebuilt}) — re-issuing live queries");
                    // Re-issuing tears down all five streams, so a snapshot
                    // refetch is needed to fill the kill→resubscribe gap. A
                    // genuine outage always refetches; a socket-healthy
                    // single-stream re-issue is rate-limited so a
                    // permanently-failing stream can't refetch the whole store
                    // every backoff cycle.
                    const REFETCH_MIN_INTERVAL: web_time::Duration =
                        web_time::Duration::from_secs(300);
                    let now = web_time::Instant::now();
                    self.kill_live_streams();
                    let refetch_due = self
                        .last_force_refetch_at
                        .map(|t| now.duration_since(t) >= REFETCH_MIN_INTERVAL)
                        .unwrap_or(true);
                    if socket_was_down {
                        self.reconnect_attempts = 0;
                    }
                    if socket_was_down || refetch_due {
                        self.force_data_refetch = true;
                        self.last_force_refetch_at = Some(now);
                    }
                    if let Some(user) = self.current_user.clone() {
                        self.load_data(ctx, &user);
                    }
                    self.needs_reconnect = false;
                    self.last_canary_at = None;
                    self.canary_sent_at = None;
                    self.canary_nonce = None;
                }
                ReconnectOutcome::AuthLost => {
                    // The socket is back but the session can't be restored
                    // (expired token, no cached password). Route to login on
                    // both platforms rather than wedging behind a modal.
                    log::warn!("Reconnected but $auth can't be restored — returning to login");
                    self.end_admin_sessions("auth lost");
                    self.kill_live_streams();
                    self.needs_reconnect = false;
                    self.reconnect_attempts = 0;
                    self.current_user = None;
                    let _ = self.app_state_tx.try_send(
                        crate::app_state::AppState::NoAuth(
                            "Your session expired — please sign in again".to_string(),
                        ),
                    );
                }
                ReconnectOutcome::Failed(e) => {
                    log::warn!("Reconnect attempt {} failed: {e}", self.reconnect_attempts);
                    self.needs_reconnect = true;
                }
            }
        }

        // Stall watchdog + retry pump: a wedged attempt is abandoned, and a
        // pending retry re-fires once `trigger_live_reconnect`'s backoff allows.
        // Tier-2 rebuilds carry up to 20s jitter plus timeboxed connect/auth,
        // so they get a longer stall budget than tier-1 socket waits.
        const RECONNECT_STALL: web_time::Duration = web_time::Duration::from_secs(45);
        const RECONNECT_STALL_REBUILD: web_time::Duration = web_time::Duration::from_secs(75);
        if self.reconnect_in_progress {
            if let Some(started) = self.reconnect_started_at {
                let stall = if self.reconnect_rebuilding { RECONNECT_STALL_REBUILD } else { RECONNECT_STALL };
                if web_time::Instant::now().duration_since(started) >= stall {
                    log::warn!("Reconnect attempt stalled for {stall:?} — abandoning it");
                    self.reconnect_in_progress = false;
                    self.reconnect_started_at = None;
                    self.reconnect_rebuilding = false;
                    self.needs_reconnect = true;
                }
            }
        } else if self.needs_reconnect {
            self.trigger_live_reconnect(ctx);
        }

        // Chat streams ride the same generation/epoch as the core five; the
        // chat tab requests them once and they re-spawn after reconnects.
        if self.user_chat.wants_streams()
            && !self.chat_streams_active
            && self.live_queries_active
            && !self.reconnect_in_progress
        {
            self.spawn_chat_streams();
        }

        self.tick_live_query_health(ctx);

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
                // Void any pre-hide canary (its timeout counted hidden time)
                // and probe immediately so a WS that died while hidden is
                // detected within seconds of returning to the foreground.
                self.canary_sent_at = None;
                self.canary_nonce = None;
                self.last_canary_at = None;
            } else {
                log::info!("Tab hidden");
                self.tab_hidden_at = Some(web_time::Instant::now());
            }
        }

        if let Ok(state) = self.app_state_rx.try_recv() {
            log::info!("Got a new state: {state:?}\nbefore state: {:?}", self.state);
            if let crate::app_state::AppState::NoAuth(reason) = &state {
                // Every route back to login lands here, so sessions and live
                // streams opened under the old identity end in one place.
                self.end_admin_sessions("signed out");
                self.kill_live_streams();
                self.current_user = None;
                let toast = &mut self.toasts;
                let error_toast = crate::ui_tools::toasts::Toast {
                    kind: crate::ui_tools::toasts::ToastKind::Error,
                    text: reason.into(),
                    options: crate::ui_tools::toasts::ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                    style: ToastStyle::default(),
                    ..Default::default()
                };
                toast.add(error_toast);
            }

            self.state = state;
            ctx.request_repaint();
        }

        self.koth.receive(ctx);
        self.query_editor.receive();
        self.receive_ui_action();
        self.receive_read_state();
        self.receive_users();
        self.receive_task();
        self.receive_ai_task();
        self.receive_notes();
        self.receive_notification(ctx);
        self.stock_tables.receive(self.ui_actions_tx.clone(), ctx, frame);
        self.sales_tracker.receive(ctx);
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

            if let Some(target) = admin_tcp_toast_target(&text) {
                if self.dismissed_admin_tcp_targets.contains(target) {
                    continue;
                }
            }

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
                ..Default::default()
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
            crate::ui_tools::theme_config::sync_editor_config(&mut self.theme_config, &settings);
            crate::ui_tools::theme_config::apply_style_with_semantics(ctx, settings);
        }
    }

    /// Tier 1 waits for the SDK's socket self-heal off-thread and restores
    /// the operator's identity. After `TIER2_AFTER_FAILURES` consecutive
    /// failures the SDK's reconnect is presumed wedged on a zombie socket and
    /// tier 2 rebuilds the whole SurrealDB client instead. Outcomes report to
    /// the `reconnect_result_rx` drain (which re-issues the LIVE SELECTs).
    /// Retries with exponential backoff — a pending retry parks on
    /// `needs_reconnect` until the backoff elapses; one attempt at a time;
    /// the operator is never blocked.
    fn trigger_live_reconnect(&mut self, _ctx: &eframe::egui::Context) {
        // Tier-1 attempts before escalating to a full client rebuild.
        const TIER2_AFTER_FAILURES: u32 = 2;
        if self.reconnect_in_progress {
            return;
        }
        let Some(user) = self.current_user.clone() else {
            self.needs_reconnect = false;
            return;
        };

        // 5s → 10s → 20s → 40s → 60s cap between consecutive failures.
        let now = web_time::Instant::now();
        let backoff = web_time::Duration::from_secs((5u64 << self.reconnect_attempts.min(4)).min(60));
        if let Some(t) = self.last_live_respawn_at {
            if self.reconnect_attempts > 0 && now.duration_since(t) < backoff {
                self.needs_reconnect = true;
                return;
            }
        }

        let rebuild = self.reconnect_attempts >= TIER2_AFTER_FAILURES;
        self.reconnect_rebuilding = rebuild;
        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        self.reconnect_token = self.reconnect_token.wrapping_add(1);
        self.reconnect_in_progress = true;
        self.reconnect_started_at = Some(now);
        self.needs_reconnect = false;
        self.last_live_respawn_at = Some(now);
        log::warn!(
            "Live-query reconnect attempt {}{}",
            self.reconnect_attempts,
            if rebuild { " (tier 2: full client rebuild)" } else { "" }
        );

        let tx = self.reconnect_result_tx.clone();
        let token = self.reconnect_token;
        let expected = user.get_id();
        PlatformSpawner::spawn(async move {
            let outcome = if rebuild {
                // rebuild_database_client sleeps its own 0-20s jitter.
                match database::rebuild_database_client().await {
                    Ok(()) => match database::restore_auth_if_needed(expected).await {
                        Ok(true) => ReconnectOutcome::Ok { socket_was_down: true, rebuilt: true },
                        Ok(false) => ReconnectOutcome::AuthLost,
                        Err(e) => ReconnectOutcome::Failed(format!("post-rebuild auth check: {e}")),
                    },
                    Err(e) => ReconnectOutcome::Failed(format!("client rebuild failed: {e}")),
                }
            } else {
                match database::await_db_socket().await {
                    Ok(socket_was_down) => {
                        if socket_was_down {
                            // Every client's probes cluster on the same backoff
                            // grid, so a recovered server would otherwise be hit
                            // by simultaneous re-auth + LIVE re-registration
                            // from the whole fleet.
                            let wait = database::random_jitter(std::time::Duration::from_secs(20));
                            log::info!("Socket recovered; waiting {wait:?} before re-auth (stampede spreading)");
                            database::sleep_compat(wait).await;
                        }
                        match database::restore_auth_if_needed(expected).await {
                            Ok(true) => ReconnectOutcome::Ok { socket_was_down, rebuilt: false },
                            Ok(false) => ReconnectOutcome::AuthLost,
                            Err(e) => ReconnectOutcome::Failed(format!("auth restore failed: {e}")),
                        }
                    }
                    Err(e) => ReconnectOutcome::Failed(e.to_string()),
                }
            };
            let _ = tx.try_send((token, outcome));
        });
    }

    /// Periodic + post-reconnect live-query health probe. Writes this
    /// session's `live_query_check` canary notification and expects it back
    /// through the `notification` live stream within `CANARY_TIMEOUT`; a
    /// miss (or a failed write) means the connection is dead and triggers a
    /// reconnect.
    fn tick_live_query_health(&mut self, ctx: &eframe::egui::Context) {
        const PROBE_INTERVAL: web_time::Duration = web_time::Duration::from_secs(45);
        const CANARY_TIMEOUT: web_time::Duration = web_time::Duration::from_secs(10);

        let Some(user) = self.current_user.clone() else {
            return;
        };
        if !self.live_queries_active || self.reconnect_in_progress {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if self.tab_hidden_at.is_some() {
            return;
        }

        let now = web_time::Instant::now();

        if let Some(sent) = self.canary_sent_at {
            if now.duration_since(sent) >= CANARY_TIMEOUT {
                log::warn!("Live-query canary timed out — subscription appears dead");
                self.canary_sent_at = None;
                self.canary_nonce = None;
                self.trigger_live_reconnect(ctx);
            }
            return;
        }

        // The first canary of a stream generation waits until every LIVE
        // registration round-trip has confirmed (or a grace window passes):
        // a canary written before the notification stream is registered can
        // never echo back and would false-positive as a dead subscription.
        const REGISTRATION_GRACE: web_time::Duration = web_time::Duration::from_secs(15);
        if self.last_canary_at.is_none() {
            let all_registered = self
                .live_registered
                .load(std::sync::atomic::Ordering::SeqCst)
                >= self.live_streams_expected;
            let grace_over = self
                .live_spawned_at
                .map(|t| now.duration_since(t) >= REGISTRATION_GRACE)
                .unwrap_or(true);
            if !all_registered && !grace_over {
                ctx.request_repaint_after(web_time::Duration::from_millis(500));
                return;
            }
        }

        let due = self
            .last_canary_at
            .map(|t| now.duration_since(t) >= PROBE_INTERVAL)
            .unwrap_or(true);
        if !due {
            return;
        }

        self.canary_seq = self.canary_seq.wrapping_add(1);
        let nonce = format!("{}:{}", self.live_session_id, self.canary_seq);
        self.canary_nonce = Some(nonce.clone());
        self.canary_sent_at = Some(now);
        self.last_canary_at = Some(now);
        let uid = user.get_id();
        let session = self.live_session_id.clone();
        let seq = self.canary_seq;
        let epoch = self.live_epoch;
        let error_tx = self.live_query_error_tx.clone();
        PlatformSpawner::spawn(async move {
            match database::schema::Notification::send_live_query_canary(
                uid.clone(),
                &session,
                nonce,
            )
            .await
            {
                Err(e) => {
                    log::warn!("send_live_query_canary failed: {e:?}");
                    let _ = error_tx.try_send((epoch, format!("canary write failed: {e}")));
                }
                Ok(()) if seq % 20 == 1 => {
                    if let Err(e) =
                        database::schema::Notification::purge_stale_canaries(uid).await
                    {
                        log::debug!("purge_stale_canaries failed: {e:?}");
                    }
                }
                Ok(()) => {}
            }
        });
        ctx.request_repaint_after(CANARY_TIMEOUT + web_time::Duration::from_millis(250));
    }

    /// UI rendering only -- toasts, modals, viewports, admin notifications.
    /// Called from `fn ui` where widget creation is allowed.
    pub fn receive_shared_ui(&mut self, ctx: &eframe::egui::Context) {
        self.admin_notification_ui(ctx);
        self.handle_viewports(ctx);
        self.handle_modals(ctx);
        self.client_diagnostics_popup_ui(ctx);
        self.drain_reachability_events();
        self.connection_status_pill(ctx);
        for text in self.toasts.show(ctx) {
            if let Some(target) = admin_tcp_toast_target(&text) {
                self.dismissed_admin_tcp_targets.insert(target.to_string());
            }
        }
    }

    /// Small corner pill shown while a reconnect is in flight or pending;
    /// reconnection runs in the background and needs no operator action.
    fn connection_status_pill(&mut self, ctx: &eframe::egui::Context) {
        if !(self.reconnect_in_progress || self.needs_reconnect) {
            return;
        }
        eframe::egui::Area::new(eframe::egui::Id::new("db_status_pill"))
            .anchor(eframe::egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0])
            .order(eframe::egui::Order::Foreground)
            .show(ctx, |ui| {
                eframe::egui::Frame::new()
                    .fill(eframe::egui::Color32::from_rgb(45, 35, 20))
                    .stroke(eframe::egui::Stroke::new(
                        1.0,
                        eframe::egui::Color32::from_rgb(220, 170, 60),
                    ))
                    .corner_radius(eframe::egui::CornerRadius::same(12))
                    .inner_margin(eframe::egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(eframe::egui::Spinner::new().size(13.0));
                            ui.label(
                                eframe::egui::RichText::new("Reconnecting…")
                                    .size(12.0)
                                    .color(eframe::egui::Color32::from_rgb(240, 210, 140)),
                            );
                        });
                    });
            });
    }

    /// Slice 5: drain the background prober's results into the
    /// per-admin `reachability_cache`. Cheap — typically zero or
    /// a handful of events per frame, only the round-completion
    /// burst sees more.
    fn drain_reachability_events(&mut self) {
        let mut changed = false;
        while let Ok(event) = self.reachability_rx.try_recv() {
            self.reachability_cache.insert(event.connection_string, event.status);
            changed = true;
        }
        // Mirror into the admin console so `open_session` can read reachability
        // without a handle to SharedContext.
        if changed {
            self.web_console_layout.reachability_cache = self.reachability_cache.clone();
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
                let _ = crate::modals::tabs::display_diagnostics_page(
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
                    None,
                    false,
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

/// Host:port from an admin TCP connect toast, if the text matches.
fn admin_tcp_toast_target(text: &str) -> Option<&str> {
    text.strip_prefix("Admin TCP connect to ")?
        .split_whitespace()
        .next()
}

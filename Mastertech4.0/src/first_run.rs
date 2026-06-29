use super::{filesystem::system_info::generate_client_id, utilities::load_encrypted_user_data, app_state::MasterTechApp, tabs::github::get_github_releases};
use displays::{app_state::AppState, pages::login_page::HASH, ui_tools::{encode_style, toasts::{Toast, ToastKind, ToastOptions}, theme_config::bootstrap_startup_theme}};
use database::{schema::{CustomerData, ExtendedSeb, LiveTaskPayload, LocalSebData, TicketData, CONNECTED_CLIENT_TABLE}, websocket_url_with_room, Database, WS_CLIENT_URL};
use database::schema::GetKeysResponse;
use eframe::egui::Context;
use database::schema::RecordId;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::spawn;

/// Global once-guard so the heavy hardware-spec scan (PowerShell GPU
/// queries, registry walks for installed programs, antivirus probes) runs
/// at most one time per process. Without this, any code path that re-toggles
/// `get_settings = true` (or future code that re-enters this branch) would
/// fan out N concurrent spec-gathers, starve tokio workers, and risk
/// blocking the UI long enough to trip epaint's 10s mutex panic.
#[cfg(target_os = "windows")]
static SPECS_GATHER_STARTED: AtomicBool = AtomicBool::new(false);

/// Once-guard for spawning the direct-TCP admin listener. Bound at most
/// once per process; reentry would race on the port and leak listeners.
static TCP_LISTENER_STARTED: AtomicBool = AtomicBool::new(false);

impl MasterTechApp {
    pub fn first_run(&mut self, ctx: &Context) {
        self.context.shared_ctx.first_run = false;
        bootstrap_startup_theme(ctx);

        match load_encrypted_user_data(HASH) {
            Some(login) => {
                if cfg!(debug_assertions) {
                    log::error!("loaded data: {login:?}");
                }
                let tx = self.context.shared_ctx.db_tx.clone();
                spawn(async move {
                    let db = Database::new(login.username.clone(), login.password.clone(), None).await;
                    let db = match db {
                        Ok(d) => Ok(d),
                        Err(e) => {
                            log::warn!("Initial DB signin failed ({e}), checking connectivity...");
                            #[cfg(target_os = "windows")]
                            match crate::utilities::windows::net_adapter::ensure_internet_connected().await {
                                Ok(()) => {
                                    log::info!("Internet restored, retrying DB signin...");
                                    Database::new(login.username, login.password, None).await
                                }
                                Err(net_err) => {
                                    log::error!("No connectivity: {net_err}");
                                    Err(e)
                                }
                            }
                            #[cfg(not(target_os = "windows"))]
                            {
                                Err(e)
                            }
                        }
                    };
                    match tx.try_send(db) {
                        Ok(_) => drop(tx),
                        Err(e) => log::error!("Error sending specs: {e:?}"),
                    }
                });
            }
            None => {
                let toast = &mut self.context.shared_ctx.toasts;

                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: "Could not get login from encoded data".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                    ..Default::default()
                };
                toast.add(error_toast);
                let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth("Needs Login".to_string()));
            }
        }

    }

    /// All channel polling and state mutations -- no UI rendering.
    /// Called from `fn logic` so it runs even when the window is hidden.
    pub fn receive_logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        if self.context.shared_ctx.first_run { self.first_run(ctx); }

        // Spawn the direct-TCP admin listener as early as possible, decoupled
        // from the heavy spec-gather below. The listener only needs the
        // client RecordId, which is derived from hostname + CPU brand --
        // both cheap, neither routed through PowerShell/JSON. Previously
        // this spawn was gated on `computer_data_rx` returning Ok, so a
        // single JSON parse failure inside `get_computer_data` (e.g.
        // "Trailing characters at line 65 column 5" from the installed-
        // programs PS script on certain machines) silently prevented the
        // listener from ever binding and the firewall rule from ever being
        // added -- breaking direct-TCP admin connections for that machine.
        // `TCP_LISTENER_STARTED` keeps this exactly-once for the process.
        if !TCP_LISTENER_STARTED.swap(true, Ordering::SeqCst) {
            spawn(async move {
                // sysinfo's refresh_all inside get_client_hash is CPU-bound;
                // hop to a blocking worker so we never stall the reactor.
                let client_uuid = match tokio::task::spawn_blocking(
                    || crate::filesystem::get_client_hash().id,
                ).await {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!(
                            "Direct-TCP listener: failed to compute client id: {e}"
                        );
                        return;
                    }
                };
                // Create the row with client_hash + computer set before the
                // publish or any friendly_name UPSERT can create a partial one.
                crate::tcp_listener::upsert_self_identity(true).await;
                // Mirror terminal-mode's brief head-start so the WS sender has
                // a chance to upsert the connected_client row before we publish
                // local_ip + tcp_port. The publish step also retries with
                // exponential back-off, so this sleep is belt-and-suspenders.
                let head_start = if crate::tcp_listener::is_self_update_child() {
                    std::time::Duration::from_millis(500)
                } else {
                    std::time::Duration::from_secs(3)
                };
                tokio::time::sleep(head_start).await;
                spawn_direct_tcp_listener(client_uuid).await;
            });
        }

        if self.context.client_friendly_name.is_empty() {
            if let Ok(name) = self.context.friendly_name_rx.try_recv() {
                if !name.is_empty() {
                    log::info!("OA3 friendly name resolved: {name}");
                    self.context.client_friendly_name = name.clone();
                    self.context.client_title = name;
                }
            }
        }

        self.context.shared_ctx.receive_shared_logic(frame, ctx);

        // Live-query auto-respawn on SurrealDB blip.  Without this, a
        // transient DB outage (e.g. the 13:56:35 ConnectionFailed event
        // that stranded a connected client off the admin's list) would
        // leave every LIVE SELECT subscription terminated and the
        // in-memory connected_client/task/notification lists stale until
        // app restart.  `receive_shared_logic` already flips
        // `live_queries_active = false` + `needs_reconnect = true` when
        // it sees a transient-looking error on the live-query error
        // channel; here we re-call `load_data` so the spawn block in
        // ui_data/mod.rs re-issues every LIVE SELECT against the
        // (auto-reconnected) SurrealDB websocket.
        //
        // The SurrealDB SDK reconnects its websocket on its own, but
        // LIVE subscriptions are per-connection state — they're gone
        // after a reset and must be explicitly re-issued.  If the SDK
        // hasn't reconnected yet by the time this runs, the respawn
        // fails fast and trips the same error channel; the bounded(1)
        // error channel naturally rate-limits the loop so we don't
        // hammer.
        if self.context.shared_ctx.needs_reconnect {
            log::info!(
                "Mastertech4.0: respawning live queries after SurrealDB blip..."
            );
            self.context.shared_ctx.needs_reconnect = false;
            if let Some(user) = self.context.shared_ctx.current_user.clone() {
                self.context.shared_ctx.load_data(ctx, &user);
            } else {
                log::warn!(
                    "Mastertech4.0: needs_reconnect set but no current_user; \
                     deferring respawn to next login"
                );
            }
        }

        self.receive_prestashop(frame);
        self.receive_database(ctx, frame);
        self.receive_github(ctx);
        self.context.scripts_tab.process_mcp_requests();
        self.context.scripts_tab.receive();
        self.context.scripts_tab.process_mcp_completions();

        // The GUI-side WS-relay `frontend.receive()` pump is gone with
        // `tabs/websockets/mod.rs`.  Admin→client egui input now arrives
        // on the direct-TCP path (handled in `tcp_listener.rs`); no
        // per-frame pump is required from this loop.

        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                let layout = user.get_user_settings().get_ui_layout_mastertech();
                if let Ok(tree) = serde_json::from_value::<egui_dock::DockState<displays::tabs::TabId>>(layout.clone()) {
                    self.dock.tree = tree;
                } else {
                    match serde_json::from_value::<egui_dock::DockState<String>>(layout) {
                        Ok(legacy) => self.dock = displays::tabs::DockSession::from_legacy_tree(legacy),
                        Err(e) => log::error!("Could not get UI layout from user: {e:?}"),
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    use crate::filesystem::system_info::ComputerInfo;
                    // Re-gather specs on startup so GPU/RAM/other fields are never stale
                    // from a prior partial run. Guarded by a process-wide AtomicBool so
                    // we can never spawn this expensive task more than once even if
                    // `get_settings` is somehow re-asserted later in the session — the
                    // earlier `cpu.is_empty()` guard implicitly provided this protection.
                    if !SPECS_GATHER_STARTED.swap(true, Ordering::SeqCst) {
                        let specs_tx = self.context.computer_data_tx.clone();
                        let current_antivirus_tx = self.context.current_antivirus_tx.clone();
                        tokio::spawn(async move {
                            match database::schema::ComputerData::default().get_computer_data().await {
                                Ok(data) => { let _ = specs_tx.try_send(data); }
                                Err(e) => log::error!("Error getting specs: {e:?}"),
                            }
                            let detected_antivirus = tokio::task::spawn_blocking(|| {
                                crate::utilities::windows::antivirus::check_antivirus().unwrap_or_default()
                            })
                            .await
                            .unwrap_or_default();
                            log::info!("detected_antivirus: {detected_antivirus:?}");
                            let _ = current_antivirus_tx.try_send(detected_antivirus);
                        });
                    }
                }
        
                if let Some(storage) = frame.storage() {
                    self.context.ticket_data = storage.get_string("ticket_data").map_or(TicketData::default(), |f| serde_json::from_str(&f).unwrap_or_default());
                    self.context.task_data = storage.get_string("task_data").map_or(LiveTaskPayload::default(), |f| serde_json::from_str(&f).unwrap_or_default());
                    self.context.customer_data = storage.get_string("customer_data").map_or(CustomerData::default(), |f| serde_json::from_str(&f).unwrap_or_default());
                    self.context.seb_info = storage.get_string("seb_info").map_or(vec![], |f| serde_json::from_str(&f).unwrap_or_default());
                }
                
                let github_tx = self.context.github_releases_channel.0.clone();
                let client = self.context.client.clone();
        
                spawn(async move {
                    match get_github_releases(github_tx, client).await {
                        Ok(_) => log::info!("get_github_releases ran ok"),
                        Err(e) => log::error!("Error getting github releases: {e:?}"),
                    }
                });
            } 
        }

        let mut latest_egui_frame = None;
        if let Some(ref rx) = self.context.egui_frame_rx {
            while let Ok(frame) = rx.try_recv() {
                latest_egui_frame = Some(frame);
            }
        }
        if let Some(frame) = latest_egui_frame {
            // Direct-TCP is the only egui-frame transport now (the
            // WS-relay branch was removed with `tabs/websockets/mod.rs`).
            if let Ok(serialized) = bincode::serde::encode_to_vec(
                &frame,
                bincode::config::standard(),
            ) {
                let mut tagged = Vec::with_capacity(1 + serialized.len());
                tagged.push(displays::EGUI_FRAME_TAG);
                tagged.extend_from_slice(&serialized);
                crate::tcp_listener::broadcast_egui_frame(tagged);
            }
        }
        
        while let Ok(message) = self.context.rx.try_recv() {
            if let Ok(keys) = serde_json::from_str::<GetKeysResponse>(&message) {
                if !keys.webroot_key.is_empty() || !keys.superanti_key.is_empty() {
                    self.context.keys = keys;
                }
                self.context.spinner = false;
            } else {
                self.context.spinner = false;
            }
        }

        if let Ok(computer_data) = self.context.computer_data_rx.try_recv() {
            self.context.computer_data = computer_data.clone();
            for disk in &self.context.computer_data.drives {
                self.context.disk_num += 1;
                if let Some(disks_arr) = self.context.disks.as_array_mut() {
                    let disk_json = serde_json::to_value(&disk).unwrap_or_default();
                    disks_arr.push(disk_json);
                } else {
                    log::debug!("Expected self.context.drives to be an Array");
                }
            }
            if let Some(seb_inf) = &self.context.computer_data.seb_info {
                log::info!("SEB: {seb_inf:#?}");
            }

            let client_hash = generate_client_id(
                self.context.computer_data.hostname.clone(), 
                self.context.computer_data.cpu.trim().to_string()
            );

            let url_string = format!(
                "{}:{}", 
                self.context.computer_data.hostname.clone(), 
                client_hash.split_at(9).0
            );

            self.context.client_title = url_string.clone();

            self.context.url = Some(websocket_url_with_room(
                WS_CLIENT_URL,
                &url_string,
                "client",
            ));
            
            self.context.client_uuid = RecordId::new(
                CONNECTED_CLIENT_TABLE.to_string(),
                url_string.clone()
            );

            // Note: the direct-TCP admin listener is no longer spawned here.
            // It now fires at the top of `receive_logic` as soon as the
            // process starts, decoupled from `get_computer_data` (which can
            // fail on installed-programs JSON parse errors and previously
            // blocked the listener from ever starting). The WebSocket relay
            // path remains active in parallel for off-LAN admins.

            #[cfg(target_os = "windows")]
            if self.context.client_friendly_name.is_empty() {
                let fname_tx = self.context.friendly_name_tx.clone();
                let client_uuid = self.context.client_uuid.clone();
                spawn(async move {
                    use crate::filesystem::oa_serial::{get_oa_style_serial, to_oa3_13digit};
                    use crate::filesystem::customer_lookup::lookup_customer_by_serial;
                    use database::schema::utilities::query_id;
                    use database::schema::client::ConnectedClient;
                    use database::DATABASE;

                    // Ensure client_hash + computer exist before the
                    // friendly_name UPSERT below, which would otherwise create
                    // a row missing both.
                    crate::tcp_listener::upsert_self_identity(true).await;

                    // Skip the PrestaShop/Everest network roundtrip when
                    // the DB already has a cached friendly_name for this
                    // OA3 — the product key is hardware-derived and won't
                    // change between sessions, so a prior successful
                    // lookup is authoritative until an admin clears it.
                    if let Ok(Some(cached)) = query_id::<ConnectedClient>(
                        CONNECTED_CLIENT_TABLE.to_string(),
                        client_uuid.clone(),
                    )
                    .await
                    {
                        if let Some(name) = cached.friendly_name.filter(|s| !s.is_empty()) {
                            log::info!(
                                "first_run -> friendly_name cached in DB ({name}); \
                                 skipping OA-serial customer lookup"
                            );
                            let _ = fname_tx.try_send(name);
                            return;
                        }
                    }

                    if let Ok(raw) = get_oa_style_serial() {
                        if let Ok(serial13) = to_oa3_13digit(&raw) {
                            // Structured lookup: customer match + open
                            // service orders in one call.  Falls back to
                            // the legacy string-only Everest path if
                            // PrestaShop fails (the Everest side doesn't
                            // expose an open-service queue we trust).
                            use crate::filesystem::customer_lookup::{
                                lookup_customer_and_open_orders,
                            };
                            match lookup_customer_and_open_orders(&serial13).await {
                                Ok((match_, candidates)) => {
                                    // Persist friendly_name so subsequent
                                    // runs hit the cache check above.
                                    // UPSERT — by this point the row should already
                                    // exist (connect() UPSERTed it earlier), but
                                    // friendly_name persistence is too important to
                                    // gate on that assumption: a missed UPSERT here
                                    // means the client appears anonymously every run
                                    // because the OA3 cache check above never hits.
                                    let res = DATABASE
                                        .query(
                                            "UPSERT $id SET friendly_name = $name, \
                                             last_update = time::now()",
                                        )
                                        .bind(("id", client_uuid.clone()))
                                        .bind(("name", match_.friendly_name.clone()))
                                        .await;
                                    match res {
                                        Ok(_) => log::info!(
                                            "first_run -> persisted friendly_name {:?} \
                                             to {client_uuid:?}",
                                            match_.friendly_name
                                        ),
                                        Err(e) => log::warn!(
                                            "first_run -> failed to persist friendly_name \
                                             to {client_uuid:?}: {e}"
                                        ),
                                    }
                                    let _ = fname_tx.try_send(match_.friendly_name.clone());

                                    // Stage 2: stash the resolution in the
                                    // in-memory cache so a later
                                    // `Cmd::RequestOpenServiceCandidates`
                                    // from the admin can be answered
                                    // without re-hitting PrestaShop.  The
                                    // admin's "Refresh suggestions" button
                                    // (Stage 3) overwrites this via the
                                    // same code path.
                                    use crate::filesystem::customer_lookup::{
                                        set_open_service_cache,
                                        CachedOpenServiceLookup,
                                    };
                                    log::info!(
                                        "first_run -> OA3 match: customer={} (id={}), \
                                         open candidates={}",
                                        match_.friendly_name,
                                        match_.id_customer,
                                        candidates.len()
                                    );
                                    for c in &candidates {
                                        log::info!(
                                            "first_run -> candidate: #{} [{}] state={} \
                                             ({}) checkin={:?}",
                                            c.service_number,
                                            c.doc_alias,
                                            c.state_name,
                                            c.state_id,
                                            c.checkin_notes
                                        );
                                    }
                                    set_open_service_cache(CachedOpenServiceLookup {
                                        match_: Some(match_),
                                        candidates,
                                        resolved_at: std::time::SystemTime::now(),
                                    });
                                }
                                Err(e) => {
                                    log::warn!(
                                        "first_run -> structured PrestaShop lookup \
                                         failed: {e:?} — falling back to Everest \
                                         friendly_name only"
                                    );
                                    if let Ok(name) =
                                        lookup_customer_by_serial(&serial13).await
                                    {
                                        // UPSERT for the same reason as the structured
                                        // PrestaShop branch above — Everest is the
                                        // fallback, so the only chance to persist a
                                        // friendly_name on this run is here.
                                        let res = DATABASE
                                            .query(
                                                "UPSERT $id SET friendly_name = $name, \
                                                 last_update = time::now()",
                                            )
                                            .bind(("id", client_uuid.clone()))
                                            .bind(("name", name.clone()))
                                            .await;
                                        if let Err(e) = res {
                                            log::warn!(
                                                "first_run -> Everest-fallback persist \
                                                 failed for {client_uuid:?}: {e}"
                                            );
                                        }
                                        let _ = fname_tx.try_send(name);
                                    }
                                }
                            }
                        }
                    }
                });
            }
        }

        if let Ok(antivirus) = self.context.current_antivirus_rx.try_recv() {
            self.context.current_antivirus = antivirus.join("\n");
        }

        if let Ok(keys) = self.context.cps_keys_rx.try_recv() {
            ctx.request_repaint();
            let key = keys.get(0).cloned().unwrap_or_default();
            if key.webroot_key.contains("Error") {
                let toast = &mut self.context.shared_ctx.toasts;
                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                    ..Default::default()
                };
                toast.add(error_toast);
            }
            self.context.keys = key;
        }

        while let Ok(copied_items) = self.context.copied_items_rx.try_recv() {
            log::info!("{copied_items}\n");
            ctx.request_repaint();
        }

        if let Ok(seb) = self.context.seb_channel.1.try_recv() {
            self.context.seb_info = seb.clone();
            let carbonite = seb.get(0).cloned().unwrap_or_default();
            self.context.computer_data.seb_info = Some(LocalSebData {
                InstalledDeviceId: carbonite.device_id.clone(),
                InstallInstanceId: carbonite.device_id.clone(),
                ActivationCode: carbonite.activation_code.clone(),
                InstallVersion: carbonite.client_version.clone(),
                MachineName: carbonite.device_name.clone(),
                ExtendedSeb: Some(ExtendedSeb {
                    email: carbonite.email.clone(),
                    phone: carbonite.phone.clone(),
                    userid: carbonite.userid.clone(),
                    device_name: carbonite.device_name.clone(),
                    device_id: carbonite.device_id.clone(),
                    state: carbonite.state.clone(),
                    usage_gb: carbonite.usage_gb.clone(),
                    date_device_created: carbonite.date_device_created.clone(),
                    activated: carbonite.activated.clone(),
                    activation_code: carbonite.activation_code.clone(),
                    last_complete_backup: carbonite.last_complete_backup.clone(),
                    last_client_status_update: carbonite.last_client_status_update.clone(),
                    id_recurly_account: carbonite.id_recurly_account.clone(),
                    date_last_scan: carbonite.date_last_scan.clone(),
                    date_email_sent: carbonite.date_email_sent.clone(),
                    date_canceled_account: carbonite.date_canceled_account.clone(),
                    date_deleted_account: carbonite.date_deleted_account.clone(),
                    current_period_ends_at: carbonite.current_period_ends_at.clone(),
                    date_modified: carbonite.date_modified.clone(),
                    date_created: carbonite.date_created.clone(),
                }),
                ..Default::default()
            });
            ctx.request_repaint();
        }
    }

    /// UI rendering only -- viewports, windows, toasts, modals.
    /// Called from `fn ui` where widget creation is allowed.
    pub fn receive_ui(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        self.viewport_loader(ctx);
        self.context.shared_ctx.receive_shared_ui(ctx);

        let theme_res = eframe::egui::Window::new("Theme Configuration")
        .open(&mut self.context.shared_ctx.modify_theme)
        .max_height(600.)
        .min_width(700.)
        .title_bar(true)
        .show(ctx, |ui|
            self.context.shared_ctx.theme_config.edit_ui(ui, ctx, self.context.shared_ctx.settings_sender.clone())
        );
        
        if let Some(window_res) = theme_res {
            if let Some(r) = window_res.inner {
                if r.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.set_color_scheme(encode_style(&r.1).unwrap_or_default());
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&r.1).unwrap_or_default());
                        }
                    }
                    self.context.shared_ctx.theme = r.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            }
        }
    }
}

/// Delegates to the shared [`crate::tcp_listener::spawn_direct_tcp_listener`].
/// Kept as a thin wrapper so the call site above doesn't need changing.
async fn spawn_direct_tcp_listener(client_uuid: RecordId) {
    crate::tcp_listener::spawn_direct_tcp_listener(client_uuid).await;
}

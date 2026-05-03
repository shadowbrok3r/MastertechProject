use super::{filesystem::system_info::generate_client_id, utilities::load_encrypted_user_data, app_state::MasterTechApp, tabs::github::get_github_releases};
use displays::{app_state::AppState, pages::login_page::HASH, ui_tools::{encode_style, toasts::{Toast, ToastKind, ToastOptions}}};
use database::{schema::{CustomerData, ExtendedSeb, LiveTaskPayload, LocalSebData, TicketData, CONNECTED_CLIENT_TABLE}, websocket_url_with_room, Database, WS_CLIENT_URL};
use database::schema::GetKeysResponse;
use eframe::egui::{Context, Style};
use database::schema::RecordId;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::spawn;

/// Global once-guard so the heavy hardware-spec scan (PowerShell GPU
/// queries, registry walks for installed programs, antivirus probes) runs
/// at most one time per process. Without this, any code path that re-toggles
/// `get_settings = true` (or future code that re-enters this branch) would
/// fan out N concurrent spec-gathers, starve tokio workers, and risk
/// blocking the UI long enough to trip epaint's 10s mutex panic.
static SPECS_GATHER_STARTED: AtomicBool = AtomicBool::new(false);

/// Once-guard for spawning the direct-TCP admin listener. Bound at most
/// once per process; reentry would race on the port and leak listeners.
static TCP_LISTENER_STARTED: AtomicBool = AtomicBool::new(false);

impl MasterTechApp {
    pub fn first_run(&mut self, ctx: &Context) {
        self.context.shared_ctx.first_run = false;
        // let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        match serde_json::from_str::<Style>(displays::STYLE) {
            Ok(theme) => {
                let style = Arc::new(theme);
                ctx.set_style(style);
            }
            Err(e) => log::error!("Error setting theme: {e:?}")
        };

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
        self.receive_prestashop(frame);
        self.receive_database(ctx, frame);
        self.receive_github(ctx);
        self.context.scripts_tab.process_mcp_requests();
        self.context.scripts_tab.receive();
        self.context.scripts_tab.process_mcp_completions();

        // Pump WebSocket receive even when the Web Console tab / viewport is closed.
        // Otherwise auto-connected clients never drain `ws_receiver` and admin→client
        // egui input (EGUI_INPUT_TAG) is never processed.
        if let Some(ref mut frontend) = self.context.frontend {
            let _ = frontend.receive();
        }

        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<egui_dock::DockState<String>>(user.get_user_settings().get_ui_layout_mastertech()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => log::error!("Could not get UI layout from user: {e:?}"),
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
                            let installed_antivirus = database::schema::ComputerData::get_antivirus().await.unwrap_or_default();
                            log::info!("installed_antivirus: {installed_antivirus:?}");
                            let _ = current_antivirus_tx.try_send(installed_antivirus);
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

        if self.context.shared_ctx.current_user.is_some()
            && !self.context.ws_auto_connected
            && self.context.frontend.is_none()
        {
            self.context.ws_auto_connected = true;
            log::info!("Auto-connecting to WebSocket server...");
            self.context.connect(ctx.clone());
        }

        let mut latest_egui_frame = None;
        if let Some(ref rx) = self.context.egui_frame_rx {
            while let Ok(frame) = rx.try_recv() {
                latest_egui_frame = Some(frame);
            }
        }
        if let Some(frame) = latest_egui_frame {
            if let Some(ref mut frontend) = self.context.frontend {
                if let Ok(serialized) = bincode::serde::encode_to_vec(
                    &frame,
                    bincode::config::standard(),
                ) {
                    let mut tagged = Vec::with_capacity(1 + serialized.len());
                    tagged.push(displays::EGUI_FRAME_TAG);
                    tagged.extend_from_slice(&serialized);
                    frontend.ws_sender.send(ewebsock::WsMessage::Binary(tagged));
                }
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

            // Spawn the direct-TCP admin listener once per process. The
            // WebSocket relay path remains active in parallel so admins
            // older than this build (or remote, off-LAN admins) keep
            // working. The admin console will prefer TCP when
            // `local_ip` + `tcp_port` are both populated on the
            // `connected_client` row.
            if !TCP_LISTENER_STARTED.swap(true, Ordering::SeqCst) {
                let client_uuid = self.context.client_uuid.clone();
                spawn(async move {
                    spawn_direct_tcp_listener(client_uuid).await;
                });
            }

            #[cfg(target_os = "windows")]
            if self.context.client_friendly_name.is_empty() {
                let fname_tx = self.context.friendly_name_tx.clone();
                spawn(async move {
                    use crate::filesystem::oa_serial::{get_oa_style_serial, to_oa3_13digit};
                    use crate::filesystem::customer_lookup::lookup_customer_by_serial;
                    if let Ok(raw) = get_oa_style_serial() {
                        if let Ok(serial13) = to_oa3_13digit(&raw) {
                            if let Ok(name) = lookup_customer_by_serial(&serial13).await {
                                let _ = fname_tx.try_send(name);
                            }
                        }
                    }
                    
                });
            }
        }

        if let Ok(antivirus) = self.context.current_antivirus_rx.try_recv() {
            let cps = &mut self.context.current_antivirus.clone();
            for (name, is_installed) in antivirus {
                match is_installed {
                    Some(true) => {
                        *cps += "\n";
                        *cps += &format!("{name}");
                    }
                    _ => {}
                }
            }
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

/// Bind the direct-TCP admin listener and publish its address to this
/// client's `connected_client` row. Logs and gives up on bind failure
/// rather than retrying forever — clients without a usable IP/port still
/// reach admins via the WebSocket relay.
async fn spawn_direct_tcp_listener(client_uuid: RecordId) {
    use crate::tcp_listener;
    use crate::utilities::network::{detect_local_ipv4, try_add_firewall_rule};
    use database::DATABASE;

    let local_ip = match detect_local_ipv4() {
        Some(ip) => ip,
        None => {
            log::warn!(
                "spawn_direct_tcp_listener -> no routable IPv4 detected; \
                 skipping direct-TCP listener (relay path still active)"
            );
            return;
        }
    };

    let (listener, addr) = match tcp_listener::bind_listener().await {
        Ok(pair) => pair,
        Err(e) => {
            log::warn!(
                "spawn_direct_tcp_listener -> bind failed: {e:?} \
                 (relay path still active)"
            );
            return;
        }
    };

    // Best-effort Windows firewall rule. If it fails, the OS firewall
    // popup still appears on the first inbound connection and the user
    // can click "Allow" once. We never block on this.
    #[cfg(target_os = "windows")]
    match try_add_firewall_rule(addr.port(), "Mastertech Direct TCP") {
        Ok(true) => log::info!(
            "spawn_direct_tcp_listener -> firewall rule added for port {}",
            addr.port()
        ),
        Ok(false) => log::info!(
            "spawn_direct_tcp_listener -> firewall rule not added (likely needs admin); \
             relying on Windows allow-access popup on first bind"
        ),
        Err(e) => log::warn!("spawn_direct_tcp_listener -> netsh spawn failed: {e}"),
    }
    #[cfg(not(target_os = "windows"))]
    let _ = try_add_firewall_rule;

    log::info!(
        "spawn_direct_tcp_listener -> listening on {} (advertise as {}:{})",
        addr,
        local_ip,
        addr.port()
    );

    // Publish IP+port to the client's row so admins can dial directly.
    // Use a separate task; if the DB write races with row creation
    // elsewhere we just retry a few times.
    let publish_uuid = client_uuid.clone();
    let port = addr.port();
    spawn(async move {
        let ip_string = local_ip.to_string();
        for attempt in 0..5u32 {
            let res = DATABASE
                .query(
                    "UPDATE $client SET local_ip = $ip, tcp_port = $port, last_update = time::now()",
                )
                .bind(("client", publish_uuid.clone()))
                .bind(("ip", ip_string.clone()))
                .bind(("port", port))
                .await;
            match res {
                Ok(_) => {
                    log::info!(
                        "spawn_direct_tcp_listener -> published {ip_string}:{port} to {:?}",
                        publish_uuid
                    );
                    return;
                }
                Err(e) => {
                    log::warn!(
                        "spawn_direct_tcp_listener -> publish attempt {} failed: {e:?}",
                        attempt + 1
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(2_u64.pow(attempt))).await;
                }
            }
        }
        log::error!(
            "spawn_direct_tcp_listener -> failed to publish IP/port after 5 attempts; \
             admins will fall back to relay"
        );
    });

    tcp_listener::accept_loop(listener).await;
}

use crate::{Cmd, PlatformSpawner, Spawner, channel_manager::ChannelManager, tabs::{ai_playground::enhanced::EnhancedAiPlayground, tasks::task_layout::{SortField, SortOptions}}, ui_tools::toasts::{Toast, ToastOptions, ToastStyle}, virtual_filesystem::FileSystem};
use eframe::egui::{self, Align, CentralPanel, Color32, Context, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui, Vec2};
use database::schema::{utilities::get_connected_clients, ConnectedClient, RecordIdExt, Sortable};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap};
use client_interface::WebSocketClient;
use crate::app_state::SharedContext;
use crate::tabs::tasks::client_cards::should_show_connected_client_in_summaries;
use crate::ui_tools::icons::{self, menu_label};
use client_action::ClientUiAction;
use client_interface::TransportKind;
use serde::Serialize;
use log::info;

use super::script_editor::ScriptEditor;
use crate::tabs::admin_console::ui::CLIENT_ROW_CONTENT_W;

pub mod client_action;
pub mod client_interface;
#[cfg(not(target_arch = "wasm32"))]
pub mod preboot_direct;
#[cfg(not(target_arch = "wasm32"))]
pub mod preboot_viewer;
pub mod relink_popup;
pub mod ui;

pub use relink_popup::RelinkClientPopup;

/// Controls whether a remote-client session is shown inline (docked in the
/// central panel when it is also the focused client) or in its own floating
/// OS viewport / egui Window.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SessionLayout {
    /// Show in the admin-console central panel.  Only one docked client is
    /// rendered at a time; whichever one matches `AdminConsole::focused_client`
    /// is the active one that receives keyboard input and plugin commands.
    #[default]
    Docked,
    /// Render in a separate OS viewport (native) or egui Window (WASM).
    Floating,
}

/// Multi-client batch action awaiting operator confirmation. The
/// Batch ▾ menu picks one of these; the confirm dialog reads it +
/// the current set of open `ws_clients`, and on Confirm fans the
/// underlying Cmd out to every client.
///
/// Each variant maps to a single `Cmd` (see
/// `dispatch_batch_action`). The mapping is one place so it's
/// easy to add new actions without rewriting the menu + dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchAction {
    Reboot,
    SwitchToTerminalMode,
    Lock,
    LogOff,
    Shutdown,
    /// Windows Update — installs and **does not** reboot. A
    /// future iteration can add a `RunWindowsUpdateAndReboot`
    /// variant if you want a "patch and bounce" all-in-one.
    RunWindowsUpdate,
}

impl BatchAction {
    /// Operator-facing label used in the Batch menu and the
    /// confirm dialog header.
    pub fn label(self) -> &'static str {
        match self {
            BatchAction::Reboot => "Reboot",
            BatchAction::SwitchToTerminalMode => "Switch to Terminal Mode",
            BatchAction::Lock => "Lock workstation",
            BatchAction::LogOff => "Log off user",
            BatchAction::Shutdown => "Shutdown",
            BatchAction::RunWindowsUpdate => "Run Windows Update",
        }
    }
}

/// Which optional right-side panel is open alongside the central client view.
#[derive(Serialize, Clone, Copy, PartialEq)]
pub enum RightPanel {
    ScriptEditor,
    Chat,
}

/// Renders one client row. Takes each field separately so callers can pass
/// disjoint borrows of `AdminConsole` while its `clients` vec is borrowed.
#[allow(clippy::too_many_arguments)]
fn render_client_row(
    ui: &mut eframe::egui::Ui,
    client: &ConnectedClient,
    ws_clients: &HashMap<String, WebSocketClient>,
    security_inventory: &HashMap<String, Vec<database::schema::InstalledSecurityProduct>>,
    session_layout: &HashMap<String, SessionLayout>,
    focused_client: Option<&str>,
    actions_tx: &Sender<ClientUiAction>,
    fk_health_tx: &crossbeam::channel::Sender<(String, bool, bool)>,
    fk_health_cache: &HashMap<String, (bool, bool)>,
    reachability: Option<&crate::ui_data::reachability::ReachabilityStatus>,
) {
    ui.add_space(4.);
    let session = ws_clients.get(&client.connection_string);
    // TCP and relay-tunnel sessions prove liveness in-band, not via WS pongs.
    let is_ws_connected = session
        .map(|wsc| {
            if wsc.transport.kind() != TransportKind::WebSocket {
                wsc.is_connected
            } else {
                wsc.is_connected && wsc.last_pong_time.is_some()
            }
        })
        .unwrap_or(false);
    let transport = session.map(|w| (w.transport.kind(), w.is_connected));
    let inventory = security_inventory
        .get(&client.connection_string)
        .map(|v| v.as_slice());
    AdminConsole::client_header(
        ui,
        actions_tx.clone(),
        client,
        session_layout.clone(),
        focused_client,
        is_ws_connected,
        fk_health_tx,
        fk_health_cache,
        inventory,
        reachability,
        transport,
    );
}

/// True only when the signed-in user's authorization is exactly `Root`.
/// Gates connecting to clients outside the live query's user/store scope.
pub fn current_user_is_root() -> bool {
    crate::get_current_user_from_auth()
        .map(|u| {
            u.get_authorization() == database::schema::user::UserAuthorization::Root
        })
        .unwrap_or(false)
}

#[derive(Serialize)]
pub struct AdminConsole {
    pub client_map: BTreeMap<String, Vec<ConnectedClient>>,
    pub clients: Vec<ConnectedClient>,
    pub search_inputs: HashMap<String, String>,
    open_menu: bool,
    #[serde(skip)]
    pub ui_actions_channel: (Sender<ClientUiAction>, Receiver<ClientUiAction>),
    /// Open right-side panel (Script Editor / Chat), or `None` when closed.
    right_panel: Option<RightPanel>,
    pub sort_by: HashMap<String, SortOptions>,
    pub last_sort_field: Option<SortField>,
    pub loading: bool,
    /// Per-session display mode.  Each entry maps a `connection_string` to
    /// either `Docked` (show in the central panel) or `Floating` (own window).
    /// Replaces the old `undock_client: HashMap<String, bool>` whose `bool`
    /// meaning was inverted and confusing.
    pub session_layout: HashMap<String, SessionLayout>,
    /// The connection string of the machine that currently has keyboard /
    /// script / plugin-command focus.  `None` when no session is open.
    pub focused_client: Option<String>,
    #[serde(skip)]
    pub filesystem: FileSystem,
    #[serde(skip)]
    pub ws_clients: HashMap<String, WebSocketClient>,
    /// Map of `connection_string` -> open `diagnostic_session.id` for any
    /// client that an AI agent is currently diagnosing through the MCP
    /// bridge. Populated when `create_diagnostic_session` succeeds and
    /// cleared when `close_diagnostic_session` runs. Read by the My Tasks
    /// connected-client cards to show an "AI active" badge.
    #[serde(skip)]
    pub active_diagnostic_sessions: HashMap<String, String>,
    /// In-memory cache of the latest security inventory we received
    /// from each connected client (slice 2 of the AV refactor).
    /// Keyed by `connection_string`. Populated by the
    /// `Cmd::SecurityInventoryResponse` handler on the admin side
    /// every time a session is opened and the client replies. Also
    /// persisted to the linked `computer` row's `current_antivirus`
    /// field via a `db().query("UPDATE …")`, so a later session
    /// on a different admin still sees the data; the in-memory copy
    /// just lets the expanded client-row body render without a DB
    /// round trip per frame.
    #[serde(skip)]
    pub security_inventory: HashMap<String, Vec<database::schema::InstalledSecurityProduct>>,
    /// Per-admin TCP reachability snapshot, mirrored from
    /// `SharedContext::reachability_cache` so `open_session` can route a
    /// probe-confirmed-unreachable client straight to the relay tunnel.
    #[serde(skip)]
    pub reachability_cache: HashMap<String, crate::ui_data::reachability::ReachabilityStatus>,
    /// Root-only connect-by-identifier field: a `connection_string` or
    /// `client_hash` typed in to reach a client the live query never returns
    /// (another store's machine, or one assigned to a different user).
    #[serde(skip)]
    pub manual_connect_input: String,
    /// Result line shown beside [`Self::manual_connect_input`] in the menu bar.
    #[serde(skip)]
    pub manual_connect_status: String,
    /// True while a lookup is in flight, so the button can't be double-fired.
    #[serde(skip)]
    pub manual_connect_busy: bool,
    #[serde(skip)]
    pub manual_connect_tx: Sender<Result<ConnectedClient, String>>,
    #[serde(skip)]
    pub manual_connect_rx: Receiver<Result<ConnectedClient, String>>,
    /// Pending batch action awaiting operator confirmation
    /// (slice 4). The Batch ▾ menu fires items into this slot;
    /// the confirm dialog reads it, and on Confirm the dispatcher
    /// fans the underlying Cmd out to every entry in
    /// `ws_clients`. Cleared on confirm or cancel.
    #[serde(skip)]
    pub pending_batch_action: Option<BatchAction>,
    pub error: String,
    script_editor: ScriptEditor,
    pub ai_playground: EnhancedAiPlayground,
    /// Open re-link popup. `Some(_)` while the admin is searching for the
    /// correct customer to bind to a connected client (the used-machine
    /// scenario where OA-key auto-detection resolves to the wrong owner).
    /// See `relink_popup.rs`.
    #[serde(skip)]
    pub relink_popup: Option<RelinkClientPopup>,
    /// `(customer_exists, computer_exists)` per `connection_string`.
    #[serde(skip)]
    pub fk_health_cache: HashMap<String, (bool, bool)>,
    #[serde(skip)]
    pub fk_health_tx: crossbeam::channel::Sender<(String, bool, bool)>,
    #[serde(skip)]
    pub fk_health_rx: crossbeam::channel::Receiver<(String, bool, bool)>,
    /// Pre-boot terminal viewer (firmware TUI relayed over HTTP). Toggled with
    /// Ctrl+Shift+B; connects by machine serial to the axum relay. Native-only
    /// (RataguiBackend depends on ratatui, a non-wasm dependency).
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    pub preboot_viewer: Option<preboot_viewer::PreBootViewer>,
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    pub preboot_roster: preboot_viewer::PreBootRoster,
    pub preboot_open: bool,
    pub preboot_serial: String,
    pub preboot_base_url: String,
    /// Direct-link hub: TCP listener firmware dials for low-latency streaming,
    /// plus its endpoint advertisement to the relay. Started lazily on the
    /// first admin-console frame.
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    pub direct_hub: preboot_direct::DirectHub,
    /// Registry plugin id to push over a direct link.
    pub preboot_plugin_id: String,
    /// Last plugin run output rendered in the pre-boot window.
    #[serde(skip)]
    pub preboot_plugin_out: Vec<String>,
}

impl AdminConsole {
    pub fn new(client_map: BTreeMap<String, Vec<ConnectedClient>>, clients: Vec<ConnectedClient>) -> Self {
        let ui_actions_channel = ClientUiAction::create_unbounded_channel();
        let (fk_health_tx, fk_health_rx) = crossbeam::channel::unbounded();
        let (manual_connect_tx, manual_connect_rx) = crossbeam::channel::unbounded();
        Self {
            clients,
            client_map,
            search_inputs: Default::default(),
            open_menu: true,
            sort_by: Default::default(),
            last_sort_field: Default::default(),
            loading: false,
            session_layout: Default::default(),
            focused_client: None,
            filesystem: FileSystem::new(),
            ws_clients: Default::default(),
            active_diagnostic_sessions: Default::default(),
            security_inventory: Default::default(),
            reachability_cache: Default::default(),
            manual_connect_input: Default::default(),
            manual_connect_status: Default::default(),
            manual_connect_busy: false,
            manual_connect_tx,
            manual_connect_rx,
            pending_batch_action: None,
            ui_actions_channel,
            error: Default::default(),
            right_panel: None,
            script_editor: ScriptEditor::new(),
            ai_playground: EnhancedAiPlayground::default(),
            relink_popup: None,
            fk_health_cache: HashMap::new(),
            fk_health_tx,
            fk_health_rx,
            #[cfg(not(target_arch = "wasm32"))]
            preboot_viewer: None,
            #[cfg(not(target_arch = "wasm32"))]
            preboot_roster: preboot_viewer::PreBootRoster::default(),
            preboot_open: false,
            preboot_serial: String::new(),
            preboot_base_url: "https://axum.master-tech.app".to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            direct_hub: preboot_direct::DirectHub::new(),
            preboot_plugin_id: "com.mastertech.uefi-diag".to_string(),
            preboot_plugin_out: Vec::new(),
        }
    }

    /// Self-contained pre-boot viewer window. Ctrl+Shift+B toggles it; enter a
    /// machine serial + Connect to poll that firmware session from the relay.
    /// Native-only (RataguiBackend/ratatui isn't a wasm dependency).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preboot_window(&mut self, ctx: &Context) {
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::B)) {
            self.preboot_open = !self.preboot_open;
        }
        if !self.preboot_open {
            return;
        }
        // Connected UEFI apps are Root-only.
        let is_root = crate::get_current_user_from_auth().map(|u| u.is_admin()).unwrap_or(false);
        let mut open = true;
        egui::Window::new(format!("{} Pre-Boot Viewer", icons::TERMINAL))
            .id(egui::Id::new("admin_preboot_viewer"))
            .default_size([920.0, 640.0])
            .open(&mut open)
            .show(ctx, |ui| {
                if !is_root {
                    ui.label(RichText::new("Root access required.").weak());
                    return;
                }
                ui.label(RichText::new("Connected UEFI apps").strong());
                let base = self.preboot_base_url.trim().trim_end_matches('/').to_string();
                if let Some(serial) = self.preboot_roster.ui(ui, &base) {
                    self.preboot_serial = serial.clone();
                    self.preboot_viewer = Some(preboot_viewer::PreBootViewer::new(serial, base.clone()));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Relay:");
                    ui.add(egui::TextEdit::singleline(&mut self.preboot_base_url).desired_width(220.0));
                    ui.label("Serial:");
                    ui.add(egui::TextEdit::singleline(&mut self.preboot_serial).desired_width(160.0));
                    let ready = !self.preboot_serial.trim().is_empty()
                        && !self.preboot_base_url.trim().is_empty();
                    if ui.add_enabled(ready, egui::Button::new("Connect")).clicked() {
                        self.preboot_viewer = Some(preboot_viewer::PreBootViewer::new(
                            self.preboot_serial.trim().to_string(),
                            self.preboot_base_url.trim().trim_end_matches('/').to_string(),
                        ));
                    }
                });
                ui.separator();
                // Plugin push over a direct link (no HTTP): only when the
                // active viewer is direct-linked to a connected box.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some((hub, serial)) =
                    self.preboot_viewer.as_ref().and_then(|v| v.direct_target())
                {
                    if let Some(res) = hub.take_plugin_result(&serial) {
                        self.preboot_plugin_out.clear();
                        if res.ok {
                            self.preboot_plugin_out.push(format!("{} v{}", res.name, res.version));
                            self.preboot_plugin_out.push(format!("tool: {}", res.tool));
                            self.preboot_plugin_out.push(res.result);
                            for l in res.log.into_iter().take(6) {
                                self.preboot_plugin_out.push(format!("log: {l}"));
                            }
                        } else {
                            self.preboot_plugin_out.push(format!("error: {}", res.error));
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Plugin:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.preboot_plugin_id)
                                .desired_width(220.0),
                        );
                        if ui.button(format!("{} Push + run", icons::PLAY)).clicked() {
                            let req = tcp_protocol::preboot::PbPluginRun {
                                source: self.preboot_plugin_id.trim().to_string(),
                                tool: String::new(),
                                args: "{}".to_string(),
                            };
                            if hub.run_plugin(&serial, &req) {
                                self.preboot_plugin_out = vec!["running…".to_string()];
                            } else {
                                self.preboot_plugin_out = vec!["push failed (not connected)".to_string()];
                            }
                        }
                    });
                    if !self.preboot_plugin_out.is_empty() {
                        egui::ScrollArea::vertical().max_height(120.0).id_salt("preboot_plugin_out").show(
                            ui,
                            |ui| {
                                for l in &self.preboot_plugin_out {
                                    ui.label(RichText::new(l).monospace().small());
                                }
                            },
                        );
                    }
                    ui.separator();
                }
                if let Some(v) = self.preboot_viewer.as_mut() {
                    v.poll();
                    v.ui(ui);
                    ctx.request_repaint_after(v.repaint_after());
                } else {
                    ui.label("Enter a serial and Connect to view a booting machine's screen.");
                }
            });
        if !open {
            self.preboot_open = false;
        }
    }

    // pub fn set_filesystem(&mut self, filesystem: FileSystem) -> &mut Self {
    //     self.filesystem = filesystem.clone();
    //     self.script_editor.set_filesystem(filesystem);
    //     self
    // }

    /// Drive every client session's transport. Rendering only pumps the
    /// focused docked session, so without this an unfocused (or hidden-tab)
    /// session never drains its channels and remote commands to it stall.
    pub fn pump_sessions(&mut self, ctx: &Context) {
        // Maintain a live session for every opened client, regardless of focus.
        #[cfg(not(target_arch = "wasm32"))]
        self.ensure_sessions();
        for ws in self.ws_clients.values_mut() {
            ws.receive(ctx);
        }
    }

    pub fn receive(&mut self, ctx: &Context) {
        // Bring up the direct-link listener + endpoint advertisement once, for
        // Root operators (the only ones who see pre-boot boxes). Both calls are
        // idempotent — internal atomics make repeats no-ops.
        #[cfg(not(target_arch = "wasm32"))]
        if crate::get_current_user_from_auth().map(|u| u.is_admin()).unwrap_or(false) {
            self.direct_hub.start(preboot_direct::DIRECT_PORT);
            self.direct_hub.advertise(self.preboot_base_url.clone());
        }
        self.filesystem.receive();
        while let Ok((cs, cust_ok, comp_ok)) = self.fk_health_rx.try_recv() {
            self.fk_health_cache.insert(cs, (cust_ok, comp_ok));
        }
        while let Ok(result) = self.manual_connect_rx.try_recv() {
            self.manual_connect_busy = false;
            // Re-checked at the point of action: authorization may have changed
            // between submitting the lookup and its result arriving.
            if !current_user_is_root() {
                self.manual_connect_status = "Root authorization required".to_string();
                continue;
            }
            match result {
                Ok(client) => {
                    let cs = client.connection_string.clone();
                    self.manual_connect_status = if client.connected {
                        format!("Opening session to {cs}")
                    } else {
                        format!("Opening session to {cs} (row says disconnected)")
                    };
                    log::info!("ConnectByIdentifier -> opening session to {cs}");
                    self.session_layout
                        .entry(cs.clone())
                        .or_insert(SessionLayout::Docked);
                    self.focused_client = Some(cs.clone());
                    if self.open_session(client) {
                        self.manual_connect_input.clear();
                    } else {
                        self.manual_connect_status = format!("Could not open a session to {cs}");
                    }
                }
                Err(e) => {
                    log::warn!("ConnectByIdentifier -> {e}");
                    self.manual_connect_status = e;
                }
            }
            ctx.request_repaint();
        }
        if let Ok(action) = self.ui_actions_channel.1.try_recv() {
            self.handle_action(action);
            ctx.request_repaint();
        }

        // Slice 2 of the AV-data refactor: drain any
        // `SecurityInventoryResponse`s the per-session
        // `WebSocketClient` pumped through the global channel. We do
        // two things per event: (1) cache the in-memory copy so the
        // expanded client-row body renders without hitting the DB,
        // and (2) fire-and-forget upsert it onto the linked
        // `computer` row's `current_antivirus` field so the data
        // outlives the admin session.
        let inv_rx = crate::get_security_inventory_receiver();
        while let Ok(event) = inv_rx.try_recv() {
            log::info!(
                "AdminConsole::receive -> caching security inventory for {} ({} products)",
                event.connection_string,
                event.products.len(),
            );
            self.security_inventory
                .insert(event.connection_string.clone(), event.products.clone());

            // Find the linked computer record (if any) and upsert.
            // Doing the lookup via the cached `clients` list is
            // cheaper than a DB round-trip per response.
            let computer_id = self
                .clients
                .iter()
                .find(|c| c.connection_string == event.connection_string)
                .and_then(|c| c.computer.clone());

            if let Some(id) = computer_id {
                let products = event.products.clone();
                let cs = event.connection_string.clone();
                crate::PlatformSpawner::spawn(async move {
                    // Use a raw UPDATE so we touch only this one
                    // field — avoids reading the whole ComputerData,
                    // mutating, and re-upserting (which is racy if
                    // anything else writes the row concurrently).
                    let res: Result<_, surrealdb::Error> = database::db()
                        .query("UPDATE $id SET current_antivirus = $products")
                        .bind(("id", id))
                        .bind(("products", products))
                        .await;
                    match res {
                        Ok(_) => log::info!(
                            "Persisted security inventory for {cs} to computer row"
                        ),
                        Err(e) => log::error!(
                            "Failed to persist security inventory for {cs}: {e}"
                        ),
                    }
                });
            } else {
                // No linked computer — the data still lives in the
                // in-memory cache so the row can render, just won't
                // survive this session. Common for freshly checked-in
                // machines that haven't been linked yet.
                log::debug!(
                    "AdminConsole::receive -> no linked computer for {}; inventory is in-memory only",
                    event.connection_string,
                );
            }
            ctx.request_repaint();
        }

        // Drive the re-link popup, if open. Poll its background channel
        // first (search/payload/apply events arrive here) and then render.
        // When the admin closes the window or Apply succeeds the popup
        // returns `false` and we drop it.
        if let Some(popup) = self.relink_popup.as_mut() {
            popup.poll();
            let still_open = popup.ui(ctx);
            if !still_open {
                // Trigger a refresh so the freshly-updated friendly_name
                // shows up in the side panel without waiting for the
                // periodic poll.
                self.relink_popup = None;
                ctx.request_repaint();
            }
        }
    }

    /// Translate a `BatchAction` into a single `Cmd` value, ready
    /// to fan out to every connected client.
    ///
    /// Kept on `AdminConsole` (not free-standing) so the mapping
    /// lives next to where it's consumed; lowers the friction of
    /// adding a new variant later (one place to edit instead of
    /// two).
    fn batch_action_cmd(action: BatchAction) -> Cmd {
        match action {
            BatchAction::Reboot => Cmd::RebootSystem {
                persist_mastertech: true,
                terminal_mode: false,
            },
            BatchAction::SwitchToTerminalMode => Cmd::LaunchTerminalMode,
            BatchAction::Lock => Cmd::LockWorkstation,
            BatchAction::LogOff => Cmd::LogOffUser,
            BatchAction::Shutdown => Cmd::ShutdownSystem,
            BatchAction::RunWindowsUpdate => Cmd::RunWindowsUpdate {
                reboot_when_done: false,
            },
        }
    }

    /// Fan a confirmed batch action out across every entry in
    /// `ws_clients`. Returns `(succeeded, failed)` counts; the
    /// caller surfaces them in a status toast.
    pub fn dispatch_batch_action(&mut self, action: BatchAction) -> (usize, usize) {
        let cmd_template = Self::batch_action_cmd(action);
        // `Cmd` doesn't implement Clone, so we round-trip through
        // serialize/deserialize per recipient. The serialized
        // bytes are small for these admin actions (~10-40 bytes
        // each), so the cost is negligible compared to the
        // network round-trip we're about to do.
        let template_bytes = match bincode::serde::encode_to_vec(&cmd_template, bincode::config::standard()) {
            Ok(b) => b,
            Err(e) => {
                log::error!("dispatch_batch_action: encode template failed: {e}");
                return (0, self.ws_clients.len());
            }
        };

        let mut ok = 0usize;
        let mut err = 0usize;
        for (cs, ws) in self.ws_clients.iter() {
            let cmd: Cmd = match bincode::serde::decode_from_slice(&template_bytes, bincode::config::standard()) {
                Ok((c, _)) => c,
                Err(e) => {
                    log::error!("dispatch_batch_action: decode for {cs} failed: {e}");
                    err += 1;
                    continue;
                }
            };
            if ws.send_cmd_tx.try_send(cmd).is_ok() {
                ok += 1;
            } else {
                err += 1;
                log::warn!("dispatch_batch_action: try_send failed for {cs}");
            }
        }
        log::info!(
            "Batch '{}': dispatched ok={ok} err={err}",
            action.label()
        );
        (ok, err)
    }
}


impl SharedContext {
    pub fn admin_console(&mut self, ui: &mut Ui){
        self.web_console_layout.receive(ui.ctx());

        // Keep the OpenAI/MCP endpoint+key override in sync with the logged-in
        // user, so the chat panel authenticates from the user's saved settings
        // regardless of which app load path populated `current_user`.
        if let Some(user) = self.current_user.as_ref() {
            crate::ai::apply_mcp_settings(user);
        }

        // Drain `pending_admin_console_focus`, set by clicking
        // "Open Console" on a My Tasks client card. The action handler
        // in `receive_ui_action.rs` only used to flip
        // `pending_activate_tab` to "Admin Console" — actually opening
        // the session on the named client was never wired through, so
        // the user landed here with nothing focused. We now:
        //
        //   1. If there's already an open `ws_clients` entry, just
        //      flip `focused_client` (avoids re-dialing).
        //   2. Otherwise look the full `ConnectedClient` up by
        //      connection_string and dispatch `ConnectClient` so the
        //      transport actually connects.
        //   3. If the lookup misses (the live-data feed hasn't
        //      populated `clients` yet on this frame), re-store the
        //      pending value so the next frame retries.
        if let Some(cs) = self.pending_admin_console_focus.take() {
            if self.web_console_layout.ws_clients.contains_key(&cs) {
                self.web_console_layout.focused_client = Some(cs);
            } else if let Some(client) = self
                .web_console_layout
                .clients
                .iter()
                .find(|c| c.connection_string == cs)
                .cloned()
            {
                self.web_console_layout
                    .handle_action(ClientUiAction::ConnectClient(client));
            } else {
                // Client list isn't ready yet — wait one frame.
                self.pending_admin_console_focus = Some(cs);
            }
        }

        let inner_margin = Margin::same(1);
        let outer_margin = Margin::same(1);
        let stroke = Stroke::new(0.7_f32, Color32::from_additive_luminance(150));
        let radius = eframe::egui::CornerRadius::same(2);

        

        eframe::egui::Panel::top("Client_Top_panel")
            .frame(
                Frame::default()
                    // .fill(Color32::from_rgb(17,17,19))
                    .inner_margin(inner_margin)
                    .outer_margin(outer_margin)
                    .stroke(stroke)
                    .corner_radius(radius)
            )
            .show_separator_line(true)
            .exact_size(26.)
            .show_inside(ui, |ui |
        {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.set_height(20.);
                ui.style_mut().spacing.button_padding = Vec2::new(5.0, 1.0);
                let txt = match self.web_console_layout.open_menu {
                    false => "Show Clients ->",
                    true => "<- Hide Clients",
                };
                if ui.button(txt).clicked() {
                    self.web_console_layout.open_menu = !self.web_console_layout.open_menu;
                }

                // ── Clients menu: Refresh + Batch submenu ────────────────
                ui.menu_button("Clients", |ui| {
                    if ui.button(format!("{}  Refresh", icons::REFRESH)).clicked() {
                        self.refresh_client_list();
                        ui.close();
                    }

                // ── Batch menu (slice 4) ─────────────────────────────────
                //
                // Fires destructive actions across every currently-open
                // session. The count next to the label shows the
                // operator how many clients the action will hit; the
                // button is disabled when nothing is connected so it
                // can't be mis-clicked into a no-op.
                //
                // Each menu item just stashes the chosen action in
                // `pending_batch_action`; the confirm dialog
                // (rendered later in this top panel) is the only
                // place that actually dispatches.
                let open_count = self.web_console_layout.ws_clients.len();
                let batch_label = menu_label(&format!("Batch ({open_count})"));
                ui.add_enabled_ui(open_count > 0, |ui| {
                    ui.menu_button(
                        RichText::new(batch_label)
                            .color(if open_count > 0 {
                                Color32::from_rgb(255, 200, 50)
                            } else {
                                Color32::DARK_GRAY
                            })
                            .strong(),
                        |ui| {
                            // Header that previews the affected
                            // clients — operators get blindsided
                            // when "Reboot all" turns out to mean
                            // "the one I forgot I had open in a
                            // floating window too."
                            ui.label(
                                RichText::new(format!("Acts on {open_count} open client(s):"))
                                    .small()
                                    .color(Color32::GRAY),
                            );
                            let names: Vec<String> = self
                                .web_console_layout
                                .ws_clients
                                .values()
                                .map(|w| {
                                    w.client
                                        .friendly_name
                                        .clone()
                                        .unwrap_or_else(|| w.client.connection_string.clone())
                                })
                                .collect();
                            for name in &names {
                                ui.label(RichText::new(format!("  • {name}")).small());
                            }
                            ui.separator();

                            // Action items. Each one *stages*; the
                            // confirm dialog is the firing point.
                            for action in [
                                BatchAction::Reboot,
                                BatchAction::SwitchToTerminalMode,
                                BatchAction::Lock,
                                BatchAction::LogOff,
                                BatchAction::Shutdown,
                                BatchAction::RunWindowsUpdate,
                            ] {
                                let color = match action {
                                    BatchAction::Shutdown => Color32::LIGHT_RED,
                                    BatchAction::Reboot | BatchAction::SwitchToTerminalMode => {
                                        Color32::from_rgb(180, 180, 200)
                                    }
                                    BatchAction::RunWindowsUpdate => {
                                        Color32::from_rgb(80, 200, 255)
                                    }
                                    _ => Color32::from_rgb(180, 180, 200),
                                };
                                if ui.button(RichText::new(action.label()).color(color)).clicked() {
                                    self.web_console_layout.pending_batch_action = Some(action);
                                    ui.close();
                                }
                            }
                        },
                    );
                });
                }); // ── end Clients menu ─────────────────────────────────

                // ── Panels menu: open Script Editor / Chat as a right panel ──
                ui.menu_button("Panels", |ui| {
                    let cur = self.web_console_layout.right_panel;
                    let script_open = cur == Some(RightPanel::ScriptEditor);
                    if ui.selectable_label(script_open, "Script Editor").clicked() {
                        self.web_console_layout.right_panel =
                            if script_open { None } else { Some(RightPanel::ScriptEditor) };
                        ui.close();
                    }
                    let chat_open = cur == Some(RightPanel::Chat);
                    if ui.selectable_label(chat_open, format!("{}  Chat", icons::CHAT)).clicked() {
                        self.web_console_layout.right_panel =
                            if chat_open { None } else { Some(RightPanel::Chat) };
                        ui.close();
                    }
                });

                ui.separator();

                // ── Visibility scope ────────────────────────────────────
                // Non-root has one option, so the combo renders disabled;
                // `connected_client_live_query` clamps it regardless.
                let is_root = current_user_is_root();
                let options = crate::ui_data::ClientScope::selectable_for(is_root);
                let mut selected = self.client_scope;
                ui.label(RichText::new("Show").small().weak());
                ui.add_enabled_ui(options.len() > 1, |ui| {
                    egui::ComboBox::from_id_salt("admin_client_scope")
                        .selected_text(RichText::new(selected.label()).small())
                        .width(88.)
                        .show_ui(ui, |ui| {
                            for opt in options {
                                ui.selectable_value(&mut selected, *opt, opt.label());
                            }
                        });
                })
                .response
                .on_hover_text(if is_root {
                    "Which connected clients to subscribe to"
                } else {
                    "Root authorization required to widen this"
                });
                if selected != self.client_scope {
                    self.client_scope = selected;
                    self.client_scope_dirty = true;
                }

                // ── Root-only connect by hash ───────────────────────────
                // Reaches a client the live query never returns (another
                // store's machine, or one assigned to another user).
                if is_root {
                    ui.separator();
                    let layout = &mut self.web_console_layout;
                    let submitted = ui
                        .add_sized(
                            [180., 18.],
                            egui::TextEdit::singleline(&mut layout.manual_connect_input)
                                .hint_text("host:hash or client hash")
                                .font(egui::TextStyle::Small),
                        )
                        .on_hover_text("Root only: connect to any client by identifier")
                        .lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let ready = !layout.manual_connect_busy
                        && !layout.manual_connect_input.trim().is_empty();
                    let clicked = ui
                        .add_enabled(
                            ready,
                            egui::Button::new(
                                RichText::new(format!("{} Connect", icons::LOCK)).small(),
                            ),
                        )
                        .clicked();
                    if ready && (submitted || clicked) {
                        let query = layout.manual_connect_input.clone();
                        let _ = layout
                            .ui_actions_channel
                            .0
                            .try_send(ClientUiAction::ConnectByIdentifier(query));
                    }
                    if !layout.manual_connect_status.is_empty() {
                        let full = layout.manual_connect_status.clone();
                        let short: String = full.chars().take(40).collect();
                        ui.label(RichText::new(short).small().weak())
                            .on_hover_text(full);
                    }
                }

                // ── Active-client breadcrumb ────────────────────────────
                //
                // Until now operators had to remember which client they
                // last clicked on to know what the Admin Console's
                // central panel was talking to. We surface the focused
                // client's friendly_name + connection_string next to the
                // tab buttons so it's always at a glance.
                //
                // The breadcrumb is right-aligned in the remaining space
                // so it sits visually opposite the "Show Clients" toggle
                // on the left edge.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(focused) = self.web_console_layout.focused_client.as_deref() {
                        let lookup = self
                            .web_console_layout
                            .clients
                            .iter()
                            .find(|c| c.connection_string == focused);
                        let (name, conn) = match lookup {
                            Some(c) => (
                                c.friendly_name.clone().unwrap_or_else(|| "(unnamed)".into()),
                                c.connection_string.clone(),
                            ),
                            None => ("(unknown)".to_string(), focused.to_string()),
                        };

                        // Render right-to-left, so push them in reverse
                        // visual order: connection_string first → name →
                        // label.
                        ui.label(
                            egui::RichText::new(conn)
                                .small()
                                .color(Color32::from_rgb(160, 160, 180)),
                        );
                        ui.label(
                            egui::RichText::new(" · ")
                                .small()
                                .color(Color32::DARK_GRAY),
                        );
                        ui.label(
                            egui::RichText::new(name)
                                .small()
                                .strong()
                                .color(Color32::from_rgb(51, 255, 189)),
                        );
                        ui.label(
                            egui::RichText::new("Active:")
                                .small()
                                .color(Color32::GRAY),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("No active client")
                                .small()
                                .italics()
                                .color(Color32::DARK_GRAY),
                        );
                    }
                });
            });
        });

        eframe::egui::Panel::left("Client_Side_panel")
            .frame(
                Frame::default()
                    .fill(ui.global_style().visuals.extreme_bg_color)
                    .inner_margin(inner_margin)
                    .outer_margin(outer_margin)
                    .stroke(stroke)
                    .corner_radius(radius)
            )
            .show_separator_line(false)
            .min_size(400.)
            .max_size(500.)
            .show_animated_inside(ui, self.web_console_layout.open_menu, |ui |
        {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                // Full panel width so the list's scrollbar sits flush against
                // the panel edge; rows self-size to CLIENT_ROW_CONTENT_W.
                ui.set_min_width(CLIENT_ROW_CONTENT_W);

                // Snapshot the per-connection reachability lookup
                // *before* the mut borrow on `web_console_layout`
                // below so the row renderer below can pass it down
                // to `client_details_grid`. Cloning the
                // `ReachabilityStatus` (a small struct of bools +
                // an Instant + an optional String) is cheap; doing
                // this each frame is OK for fleets up to a few
                // hundred clients.
                let reachability_snapshot: HashMap<String, crate::ui_data::reachability::ReachabilityStatus> =
                    self.web_console_layout
                        .clients
                        .iter()
                        .filter_map(|c| {
                            self.reachability_cache
                                .get(&c.connection_string)
                                .map(|s| (c.connection_string.clone(), s.clone()))
                        })
                        .collect();

                // Snapshotted before the mut borrow below so the grouped list
                // can read them.
                let group_by_store =
                    self.client_scope == crate::ui_data::ClientScope::AllClients;
                let user_store_map = if group_by_store {
                    self.user_store_map.clone()
                } else {
                    HashMap::new()
                };

                let ws_client = &mut self.web_console_layout;
                let clients = &mut ws_client.clients;
                let sort_by = ws_client.sort_by.entry("Connected".to_string()).or_default();
                let direction = &sort_by.direction;
                match sort_by.field {
                    SortField::Default => clients.default_sort(direction.clone()),
                    SortField::Date => clients.sort_by_date(direction.clone()),
                    SortField::Name => clients.sort_by_name(direction.clone()),
                };
                // Stable secondary sort: clients assigned to the logged-in user float
                // to the top regardless of the primary sort direction.
                if let Some(me) = crate::get_current_user_from_auth() {
                    let my_id = me.get_id();
                    clients.sort_by(|a, b| {
                        let a_mine = a.assigned_user.as_ref()
                            .is_some_and(|u| u.key_string() == my_id.key_string());
                        let b_mine = b.assigned_user.as_ref()
                            .is_some_and(|u| u.key_string() == my_id.key_string());
                        b_mine.cmp(&a_mine) // mine first; equal elements keep prior order (stable)
                    });
                }
                
                let visible_indices: Vec<usize> = clients
                    .iter()
                    .enumerate()
                    .filter_map(|(i, client)| {
                        let is_ws_connected = ws_client
                            .ws_clients
                            .get(&client.connection_string)
                            .map(|wsc| {
                                // TCP and relay-tunnel sessions prove liveness
                                // in-band (ping/pong), not via ewebsock pongs.
                                if wsc.transport.kind() != TransportKind::WebSocket {
                                    wsc.is_connected
                                } else {
                                    wsc.is_connected && wsc.last_pong_time.is_some()
                                }
                            })
                            .unwrap_or(false);
                        should_show_connected_client_in_summaries(client, is_ws_connected)
                            .then_some(i)
                    })
                    .collect();

                if visible_indices.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No clients with connected = true in the database (or an open admin session).",
                        )
                        .weak(),
                    );
                    ui.add_space(6.);
                }

                // Root-only pre-boot UEFI / QC-agent rows, shown in their own
                // section under the machine list. (serial, friendly, age_secs,
                // kind, direct-linked).
                let is_root =
                    crate::get_current_user_from_auth().map(|u| u.is_admin()).unwrap_or(false);
                let mut preboot_rows: Vec<(String, String, i64, &'static str, bool)> = if is_root {
                    clients
                        .iter()
                        .filter(|c| {
                            matches!(
                                c.client_kind,
                                database::schema::client::ClientKind::QcAgent
                                    | database::schema::client::ClientKind::Uefi
                            ) && c.connected
                        })
                        .map(|c| {
                            let serial = if c.connection_string.trim().is_empty() {
                                c.id.key_string().trim_start_matches("qc_").to_string()
                            } else {
                                c.connection_string.trim().to_string()
                            };
                            let age = c
                                .last_update
                                .as_ref()
                                .and_then(|d| {
                                    chrono::DateTime::parse_from_rfc3339(&d.to_string()).ok()
                                })
                                .map(|t| {
                                    (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
                                        .num_seconds()
                                })
                                .unwrap_or(i64::MAX);
                            let kind =
                                if c.client_kind == database::schema::client::ClientKind::Uefi {
                                    "uefi"
                                } else {
                                    "qc"
                                };
                            (serial, c.friendly_name.clone().unwrap_or_default(), age, kind, false)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                // Merge direct-linked boxes: mark existing rows direct, add any
                // that are direct-only (not yet in the DB client list).
                #[cfg(not(target_arch = "wasm32"))]
                if is_root {
                    for a in ws_client.direct_hub.agents() {
                        if let Some(row) = preboot_rows.iter_mut().find(|r| r.0 == a.serial) {
                            row.3 = "uefi";
                            row.4 = true;
                            row.2 = row.2.min(a.idle_secs as i64);
                        } else {
                            preboot_rows.push((a.serial, String::new(), a.idle_secs as i64, "uefi", true));
                        }
                    }
                }
                let show_preboot = !preboot_rows.is_empty();
                // Machine list keeps the top 75%; the pre-boot section gets the rest.
                let list_max_h =
                    if show_preboot { ui.available_height() * 0.75 } else { f32::INFINITY };
                ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(list_max_h)
                    .show(ui, |ui| {
                    ui.set_min_width(CLIENT_ROW_CONTENT_W);
                    // Fleet-wide lists group under one collapsing header per
                    // owning store; every other scope is a single flat group.
                    let groups: Vec<(Option<String>, Vec<usize>)> = if group_by_store {
                        let mut by_store: std::collections::BTreeMap<String, Vec<usize>> =
                            std::collections::BTreeMap::new();
                        for &index in &visible_indices {
                            let Some(c) = clients.get(index) else { continue };
                            let store = c
                                .assigned_user
                                .as_ref()
                                .and_then(|u| user_store_map.get(&u.key_string()))
                                .cloned()
                                .unwrap_or_else(|| "Unknown store".to_string());
                            by_store.entry(store).or_default().push(index);
                        }
                        by_store.into_iter().map(|(s, v)| (Some(s), v)).collect()
                    } else {
                        vec![(None, visible_indices.clone())]
                    };

                    for (header, indices) in groups {
                        match header {
                            Some(store) => {
                                egui::CollapsingHeader::new(
                                    RichText::new(format!("{store}  ({})", indices.len()))
                                        .strong(),
                                )
                                .id_salt(("admin_store_group", store.as_str()))
                                .default_open(true)
                                .show(ui, |ui| {
                                    for &index in &indices {
                                        if let Some(client) = clients.get(index) {
                                            render_client_row(
                                                ui,
                                                client,
                                                &ws_client.ws_clients,
                                                &ws_client.security_inventory,
                                                &ws_client.session_layout,
                                                ws_client.focused_client.as_deref(),
                                                &ws_client.ui_actions_channel.0,
                                                &ws_client.fk_health_tx,
                                                &ws_client.fk_health_cache,
                                                reachability_snapshot
                                                    .get(&client.connection_string),
                                            );
                                        }
                                    }
                                });
                            }
                            None => {
                                for &index in &indices {
                                    if let Some(client) = clients.get(index) {
                                        render_client_row(
                                            ui,
                                            client,
                                            &ws_client.ws_clients,
                                            &ws_client.security_inventory,
                                            &ws_client.session_layout,
                                            ws_client.focused_client.as_deref(),
                                            &ws_client.ui_actions_channel.0,
                                            &ws_client.fk_health_tx,
                                            &ws_client.fk_health_cache,
                                            reachability_snapshot
                                                .get(&client.connection_string),
                                        );
                                    }
                                }
                            }
                        }
                    }
                });

                if show_preboot {
                    ui.add_space(4.);
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{} Pre-Boot UEFI ({})",
                            icons::TERMINAL,
                            preboot_rows.len()
                        ))
                        .strong(),
                    );
                    let mut view_serial: Option<(String, bool)> = None;
                    ScrollArea::vertical()
                        .id_salt("preboot_qc_section")
                        .auto_shrink([true, true])
                        .show(ui, |ui| {
                            ui.set_width(CLIENT_ROW_CONTENT_W);
                            for (serial, friendly, age, kind, direct) in &preboot_rows {
                                ui.add_space(4.);
                                #[cfg(not(target_arch = "wasm32"))]
                                if AdminConsole::preboot_card(ui, serial, friendly, *age, kind, *direct) {
                                    view_serial = Some((serial.clone(), *direct));
                                }
                            }
                        });
                    if let Some((s, direct)) = view_serial {
                        ws_client.preboot_serial = s.clone();
                        ws_client.preboot_open = true;
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let viewer = if direct {
                                preboot_viewer::PreBootViewer::new_direct(s, ws_client.direct_hub.clone())
                            } else {
                                let base = ws_client
                                    .preboot_base_url
                                    .trim()
                                    .trim_end_matches('/')
                                    .to_string();
                                preboot_viewer::PreBootViewer::new(s, base)
                            };
                            ws_client.preboot_viewer = Some(viewer);
                        }
                        #[cfg(target_arch = "wasm32")]
                        let _ = direct;
                    }
                }
            });
        });

        // Right panel: Script Editor / Chat, opened from the Panels menu, so an
        // operator can drive the AI (or edit a script) while watching the
        // focused client run things in the central panel.
        let right_open = self.web_console_layout.right_panel.is_some();
        eframe::egui::Panel::right("Client_Right_panel")
            .frame(
                Frame::default()
                    .fill(ui.global_style().visuals.extreme_bg_color)
                    .inner_margin(inner_margin)
                    .outer_margin(outer_margin)
                    .stroke(stroke)
                    .corner_radius(radius)
            )
            .show_separator_line(false)
            .min_size(440.)
            .max_size(900.)
            .show_animated_inside(ui, right_open, |ui| {
                match self.web_console_layout.right_panel {
                    // Chat owns its own compact top bar (threads + close), so no
                    // extra header here. Its ✕ raises a close request we drain.
                    Some(RightPanel::Chat) => {
                        self.web_console_layout.ai_playground.enhanced_ai_playground(ui);
                        if self.web_console_layout.ai_playground.take_close_request() {
                            self.web_console_layout.right_panel = None;
                        }
                    }
                    Some(RightPanel::ScriptEditor) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Script Editor").strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(RichText::new(icons::CLOSE)).on_hover_text("Close panel").clicked() {
                                    self.web_console_layout.right_panel = None;
                                }
                            });
                        });
                        ui.separator();
                        self.web_console_layout.right_panel_ui(ui);
                    }
                    None => {}
                }
            });

        CentralPanel::default().show_inside(ui, |ui| {
            let ws_layout = &mut self.web_console_layout;
            // let connection_string = ws_layout.c
            if !ws_layout.error.is_empty() {
                let options = ToastOptions::default();
                options.duration(Some(web_time::Duration::from_secs(3)));

                self.toasts.add(Toast {
                    kind: crate::ui_tools::toasts::ToastKind::Error,
                    text: ws_layout.error.clone().into(),
                    options,
                    style: ToastStyle::default(),
                    ..Default::default()
                });
                ws_layout.error.clear();
            }
            ws_layout.ui(ui);
        });

        // Pre-boot terminal viewer window (Ctrl+Shift+B), self-contained.
        #[cfg(not(target_arch = "wasm32"))]
        self.web_console_layout.preboot_window(ui.ctx());

        // ── Batch-action confirm dialog (slice 4) ─────────────────────
        //
        // Rendered at the end so it overlays everything else on this
        // tab. The Batch menu sets `pending_batch_action`; this
        // dialog is the only place the action actually fires. On
        // Confirm we call `dispatch_batch_action` and clear the
        // pending slot regardless of outcome.
        if let Some(action) = self.web_console_layout.pending_batch_action {
            let target_names: Vec<String> = self
                .web_console_layout
                .ws_clients
                .values()
                .map(|w| {
                    w.client
                        .friendly_name
                        .clone()
                        .unwrap_or_else(|| w.client.connection_string.clone())
                })
                .collect();
            let count = target_names.len();

            // Center the window so it can't be missed behind the
            // client list — destructive ops shouldn't be easy to
            // mis-click around.
            let mut still_open = true;
            egui::Window::new(format!("Confirm: {}", action.label()))
                .id(egui::Id::new("admin_batch_confirm"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .open(&mut still_open)
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(360.0);
                    ui.label(
                        RichText::new(format!(
                            "{} on {count} open client(s)?",
                            action.label()
                        ))
                        .strong(),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_width(220.0)
                        .show(ui, |ui| {
                            for name in &target_names {
                                ui.label(
                                    RichText::new(format!("• {name}"))
                                        .small()
                                        .color(Color32::from_rgb(199, 202, 245)),
                                );
                            }
                        });
                    ui.add_space(8.0);

                    // Special-case the destructive-looking
                    // warning copy so the operator pauses on
                    // Shutdown / Reboot.
                    let warning = match action {
                        BatchAction::Shutdown => Some(
                            "These machines will power off immediately. \
                             Mastertech will reconnect on next boot only \
                             if the client is set to auto-start.",
                        ),
                        BatchAction::Reboot => Some(
                            "Sessions will drop while the clients reboot. \
                             Mastertech reattaches automatically after \
                             restart.",
                        ),
                        BatchAction::SwitchToTerminalMode => Some(
                            "Spawns Mastertech in terminal mode on each client. \
                             The current GUI session will remain running.",
                        ),
                        BatchAction::LogOff => Some(
                            "User sessions will end immediately on each \
                             client. Unsaved work may be lost.",
                        ),
                        BatchAction::RunWindowsUpdate => Some(
                            "This kicks off a Windows Update scan + install \
                             on each client. The operation can take many \
                             minutes; results land in toasts as each client \
                             finishes.",
                        ),
                        BatchAction::Lock => None,
                    };
                    if let Some(text) = warning {
                        ui.label(
                            RichText::new(text)
                                .small()
                                .italics()
                                .color(Color32::from_rgb(255, 200, 120)),
                        );
                        ui.add_space(8.0);
                    }

                    ui.horizontal(|ui| {
                        let confirm_label = format!("{} all", action.label());
                        if ui
                            .button(
                                RichText::new(confirm_label)
                                    .color(Color32::from_rgb(255, 150, 80)),
                            )
                            .clicked()
                        {
                            let (ok, err) = self.web_console_layout.dispatch_batch_action(action);
                            let toast = if err == 0 {
                                crate::ToastMessage::Success(format!(
                                    "Batch '{}' sent to {ok} client(s).",
                                    action.label()
                                ))
                            } else {
                                crate::ToastMessage::Warning(format!(
                                    "Batch '{}' sent to {ok} client(s); {err} failed to dispatch.",
                                    action.label()
                                ))
                            };
                            let _ = crate::get_toast_sender().try_send(toast);
                            self.web_console_layout.pending_batch_action = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.web_console_layout.pending_batch_action = None;
                        }
                    });
                });
            // X-button on the window closes via `open` too.
            if !still_open {
                self.web_console_layout.pending_batch_action = None;
            }
        }
    }

    pub fn refresh_client_list(&mut self) {
        let tx = self.connected_clients_tx.clone();
        let scope = self.client_scope;
        PlatformSpawner::spawn(async move {
            match get_connected_clients(tx, scope).await {
                Ok(_) => info!("web_console/mod.rs -> get_connected_clients ran ok"),
                Err(e) => log::warn!("web_console/mod.rs -> get_connected_clients error: {e:?}"),
            }
        });
    }
}


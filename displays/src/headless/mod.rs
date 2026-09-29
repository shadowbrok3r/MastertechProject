//! Headless admin session engine.
//!
//! Holds admin sessions to connected clients and routes their replies into the
//! same registries the desktop console feeds, so every remote MCP tool works
//! with no GUI and no operator focus. The desktop console remains the operator
//! surface; this is the always-on one.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crossbeam::channel::Receiver;
use ewebsock::WsMessage;

use crate::tabs::admin_console::client_interface::{AdminTransport, SessionEvent};
use crate::Cmd;

mod assist;
mod assistant_jobs;
pub mod codex;
mod notify;
mod offer;
mod stress_reap;

pub use assist::spawn_assist_dispatcher;
pub use assistant_jobs::spawn_assistant_jobs;
pub use codex::spawn_codex_broker;
pub use notify::spawn_shelf_notifier;
pub use stress_reap::spawn_stress_reaper;

/// Poll interval for the session pump.
const PUMP_MS: u64 = 100;
/// How often the client roster is re-read from the database.
const ROSTER_SECS: u64 = 10;
/// Keepalive ping interval, matching the desktop console.
const PING_SECS: u64 = 15;
/// How long a connected session outside the roster stays open after the client last spoke.
const QUIET_GRACE: Duration = Duration::from_secs(120);
/// How long a session a tool asked for stays open outside the roster.
const REQUEST_HOLD: Duration = Duration::from_secs(30 * 60);

fn max_sessions() -> usize {
    std::env::var("MTECH_AGENT_MAX_SESSIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32)
}

struct Session {
    transport: AdminTransport,
    /// MCP-injected bytes bound for this client.
    inbox: Receiver<Vec<u8>>,
    connection_string: String,
    computer: Option<database::schema::RecordId>,
    last_ping: std::time::Instant,
    /// Connected since the transport's last `Opened`.
    open: bool,
    last_heard: std::time::Instant,
    /// Kept open outside the roster until then; set when a tool asked for this session.
    held_until: Option<std::time::Instant>,
}

impl Session {
    fn open(client: &database::schema::ConnectedClient) -> Option<Self> {
        let transport = AdminTransport::dial(client)?;
        let connection_string = client.connection_string.clone();
        let inbox = crate::plugins::remote_egui_control::hub().register(connection_string.clone());
        crate::plugins::remote_egui_control::hub().set_open(&connection_string, false);
        log::info!("headless: opened session -> {connection_string}");
        Some(Self {
            transport,
            inbox,
            connection_string,
            computer: client.computer.clone(),
            last_ping: std::time::Instant::now(),
            open: false,
            last_heard: std::time::Instant::now(),
            held_until: None,
        })
    }

    /// Whether to keep the session while the roster leaves its client out.
    fn keep_outside_roster(&self) -> bool {
        (self.open && self.last_heard.elapsed() < QUIET_GRACE)
            || self.held_until.is_some_and(|until| std::time::Instant::now() < until)
    }

    fn hold(&mut self) {
        self.held_until = Some(std::time::Instant::now() + REQUEST_HOLD);
    }

    fn set_open(&mut self, open: bool) {
        if self.open != open {
            self.open = open;
            crate::plugins::remote_egui_control::hub().set_open(&self.connection_string, open);
        }
    }

    /// Drains MCP-bound bytes to the client and routes replies back.
    fn pump(&mut self) {
        while let Ok(bytes) = self.inbox.try_recv() {
            self.transport.send(WsMessage::Binary(bytes));
        }
        if self.last_ping.elapsed() >= Duration::from_secs(PING_SECS) {
            self.last_ping = std::time::Instant::now();
            self.transport.send_cmd(&Cmd::AppPing {
                nonce: rand_nonce(),
                sent_at_ms: now_ms(),
            });
        }
        while let Some(event) = self.transport.poll_event() {
            if !matches!(event, SessionEvent::Closed | SessionEvent::Error(_)) {
                self.last_heard = std::time::Instant::now();
            }
            match event {
                SessionEvent::Cmd(cmd) => self.route(cmd),
                SessionEvent::Binary(bin) => self.route_binary(&bin),
                SessionEvent::Opened => {
                    log::info!("headless: {} opened", self.connection_string);
                    self.set_open(true);
                    mark_connected(&self.connection_string);
                }
                SessionEvent::Closed => {
                    log::warn!("headless: {} closed", self.connection_string);
                    self.set_open(false);
                }
                SessionEvent::Error(e) => {
                    log::warn!("headless: {} error: {e}", self.connection_string);
                    self.set_open(false);
                }
                SessionEvent::Text(_) => {}
            }
        }
        for notice in crate::plugins::crash_intel_hooks::drain_notices(&self.connection_string) {
            log::info!("headless: {}: {notice}", self.connection_string);
        }
    }

    /// Every reply route the MCP tools block on; a missing arm is a tool timeout.
    fn route(&self, cmd: Cmd) {
        use crate::plugins::{mcp_bridge, remote_script_notify};
        match cmd {
            Cmd::RemotePluginToolResult { request_id, plugin_id, tool_name, success, result_json } => {
                if success {
                    self.ingest(&plugin_id, &tool_name, &result_json);
                }
                mcp_bridge::resolve_pending_request(&request_id, success, result_json);
            }
            Cmd::LoadWasmPluginResult { plugin_id, success, message } => {
                remote_script_notify::notify_deploy_ack(&plugin_id, success, &message);
            }
            Cmd::RemoteScriptLog(msg) => {
                remote_script_notify::notify_remote_script_log(&self.connection_string, msg);
            }
            Cmd::RemoteScriptResult { name, status } => {
                remote_script_notify::notify_remote_script_result(
                    &self.connection_string,
                    name,
                    format!("{status:?}"),
                );
            }
            Cmd::RemoteScriptsComplete => {
                remote_script_notify::notify_remote_scripts_complete(&self.connection_string);
            }
            Cmd::RemoteScriptListResponse { categories } => {
                remote_script_notify::notify_script_list(categories.len());
            }
            Cmd::DirectFileTransferResult { success, message, .. } => {
                if let Some((dest, req)) =
                    mcp_bridge::take_headless_dump_fetch(&self.connection_string)
                {
                    if success {
                        mcp_bridge::resolve_pending_request(
                            &req,
                            true,
                            dest.to_string_lossy().to_string(),
                        );
                    } else {
                        mcp_bridge::resolve_pending_request(
                            &req,
                            false,
                            format!("download failed: {message}"),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Passes crash and driver results to the fleet-intel ingest hooks.
    fn ingest(&self, plugin_id: &str, tool_name: &str, result_json: &str) {
        use crate::plugins::{crash_intel_hooks as crash, driver_intel_hooks as drivers};
        let (cs, computer) = (self.connection_string.clone(), self.computer.clone());
        let (tool, json) = (tool_name.to_string(), result_json.to_string());
        let pid = plugin_id.to_string();
        if crash::is_dump_analysis_result(plugin_id, tool_name) {
            crash::ingest_dump_decode_result(cs, computer, pid, tool, json);
        } else if crash::is_kernel_triage_result(plugin_id, tool_name) {
            crash::ingest_kernel_triage_result(cs, computer, pid, tool, json);
        } else if crash::is_gpu_crash_result(plugin_id, tool_name) {
            crash::ingest_gpu_crash_result(cs, computer, pid, tool, json);
        } else if drivers::is_driver_snapshot_result(plugin_id, tool_name) {
            drivers::ingest_driver_snapshot(cs, computer, json);
        }
    }

    /// Viewer frames carry widget anchors the remote-egui tools read back.
    fn route_binary(&self, bin: &[u8]) {
        if bin.first() != Some(&crate::EGUI_FRAME_TAG) {
            return;
        }
        if let Ok(frame) =
            bincode::serde::decode_from_slice::<crate::plugins::EguiFrameMessage, _>(
                &bin[1..],
                tcp_protocol::WIRE_DECODE,
            )
        {
            crate::plugins::remote_egui_control::hub()
                .record_last_frame(&self.connection_string, &frame.0);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        crate::plugins::remote_egui_control::hub().unregister(&self.connection_string);
        crate::plugins::remote_script_notify::drop_session(&self.connection_string);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn rand_nonce() -> u64 {
    now_ms().rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15
}

fn mark_connected(connection_string: &str) {
    let cs = connection_string.to_string();
    tokio::spawn(async move {
        let res = database::db()
            .query("UPDATE connected_client SET connected = true WHERE connection_string = $cs")
            .bind(("cs", cs.clone()))
            .await;
        if let Err(e) = res {
            log::warn!("headless: mark_connected {cs} failed: {e}");
        }
    });
}

/// Newest-first so the session cap keeps the machines most likely to be live
/// rather than an arbitrary subset. No staleness bound here on purpose: the
/// heartbeat is every 15 minutes and axum_server's sweep already clears
/// `connected` past 30, whose generosity is what stops one missed write from
/// dropping a healthy agent.
async fn roster() -> Vec<database::schema::ConnectedClient> {
    let sql = "SELECT * FROM connected_client \
               WHERE connected = true AND client_kind = 'machine' \
               ORDER BY last_update DESC LIMIT $cap";
    match database::db().query(sql).bind(("cap", max_sessions() as i64)).await {
        Ok(mut res) => res.take(0).unwrap_or_default(),
        Err(e) => {
            log::warn!("headless: roster query failed: {e}");
            Vec::new()
        }
    }
}

/// The client row for `connection_string`, whatever its `connected` flag says.
async fn client_row(connection_string: &str) -> Option<database::schema::ConnectedClient> {
    let sql = "SELECT * FROM connected_client WHERE connection_string = $cs LIMIT 1";
    match database::db().query(sql).bind(("cs", connection_string.to_string())).await {
        Ok(mut res) => res.take::<Vec<database::schema::ConnectedClient>>(0).ok()?.into_iter().next(),
        Err(e) => {
            log::warn!("headless: client lookup for {connection_string} failed: {e}");
            None
        }
    }
}

/// Opens or holds the sessions tools asked for, whether or not the roster lists their clients.
async fn open_requested(sessions: &mut HashMap<String, Session>) {
    for cs in crate::plugins::remote_egui_control::hub().take_requested() {
        if let Some(session) = sessions.get_mut(&cs) {
            session.hold();
            continue;
        }
        let Some(client) = client_row(&cs).await else {
            log::warn!("headless: a tool asked for {cs}, which has no connected_client row");
            continue;
        };
        if let Some(mut session) = Session::open(&client) {
            log::info!("headless: opened requested session -> {cs}");
            session.hold();
            sessions.insert(cs, session);
        }
    }
}

/// Runs the session pump until cancelled; never returns under normal operation.
pub async fn run_session_engine() {
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut last_roster = std::time::Instant::now() - Duration::from_secs(ROSTER_SECS);
    let toasts = crate::get_toast_receiver();
    crate::plugins::remote_egui_control::hub().set_dialer();

    loop {
        while let Ok(toast) = toasts.try_recv() {
            log::info!("headless: {toast:?}");
        }
        open_requested(&mut sessions).await;
        if last_roster.elapsed() >= Duration::from_secs(ROSTER_SECS) {
            last_roster = std::time::Instant::now();
            let clients = roster().await;
            let live: Vec<String> = clients.iter().map(|c| c.connection_string.clone()).collect();
            sessions.retain(|cs, s| !s.transport.is_closed() && (live.contains(cs) || s.keep_outside_roster()));
            for client in clients {
                if client.connection_string.is_empty() || sessions.contains_key(&client.connection_string) {
                    continue;
                }
                if sessions.len() >= max_sessions() {
                    break;
                }
                if let Some(session) = Session::open(&client) {
                    sessions.insert(client.connection_string.clone(), session);
                    let (cs, computer) = (client.connection_string.clone(), client.computer.clone());
                    tokio::spawn(async move {
                        offer::offer_for(&cs, computer.as_ref()).await;
                    });
                }
            }
        }
        for session in sessions.values_mut() {
            session.pump();
        }
        tokio::time::sleep(Duration::from_millis(PUMP_MS)).await;
    }
}

/// Boots the MCP server and the session pump together.
pub async fn run(mcp_http: bool) -> anyhow::Result<()> {
    let (dispatcher, _cmd_rx) = crate::plugins::DefaultEventDispatcher::new();
    let manager = {
        let mut mgr = crate::plugins::PluginManager::new();
        mgr.set_dispatcher(dispatcher);
        Arc::new(std::sync::RwLock::new(mgr))
    };

    tokio::spawn(run_session_engine());
    codex::require_system_session().await;
    codex::require_write_role().await;
    spawn_codex_broker(manager.clone());
    spawn_assist_dispatcher();
    spawn_shelf_notifier();
    spawn_stress_reaper();
    spawn_assistant_jobs();

    if mcp_http {
        crate::plugins::run_plugin_mcp_server_http(manager).await
    } else {
        crate::plugins::run_plugin_mcp_server(manager).await
    }
}

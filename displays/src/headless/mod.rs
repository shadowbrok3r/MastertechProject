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
mod chat;
mod notify;
mod offer;

pub use assist::spawn_assist_dispatcher;
pub use chat::spawn_chat_bridge;
pub use notify::spawn_shelf_notifier;

/// Poll interval for the session pump.
const PUMP_MS: u64 = 100;
/// How often the client roster is re-read from the database.
const ROSTER_SECS: u64 = 10;
/// Keepalive ping interval, matching the desktop console.
const PING_SECS: u64 = 15;

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
    last_ping: std::time::Instant,
}

impl Session {
    fn open(client: &database::schema::ConnectedClient) -> Option<Self> {
        let transport = AdminTransport::dial(client)?;
        let connection_string = client.connection_string.clone();
        let inbox = crate::plugins::remote_egui_control::hub().register(connection_string.clone());
        log::info!("headless: opened session -> {connection_string}");
        Some(Self { transport, inbox, connection_string, last_ping: std::time::Instant::now() })
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
            match event {
                SessionEvent::Cmd(cmd) => self.route(cmd),
                SessionEvent::Binary(bin) => self.route_binary(&bin),
                SessionEvent::Opened => {
                    log::info!("headless: {} opened", self.connection_string);
                    mark_connected(&self.connection_string);
                }
                SessionEvent::Closed => log::warn!("headless: {} closed", self.connection_string),
                SessionEvent::Error(e) => log::warn!("headless: {} error: {e}", self.connection_string),
                SessionEvent::Text(_) => {}
            }
        }
    }

    /// Every reply route the MCP tools block on; a missing arm is a tool timeout.
    fn route(&self, cmd: Cmd) {
        use crate::plugins::{mcp_bridge, remote_script_notify};
        match cmd {
            Cmd::RemotePluginToolResult { request_id, success, result_json, .. } => {
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

/// Runs the session pump until cancelled; never returns under normal operation.
pub async fn run_session_engine() {
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut last_roster = std::time::Instant::now() - Duration::from_secs(ROSTER_SECS);

    loop {
        if last_roster.elapsed() >= Duration::from_secs(ROSTER_SECS) {
            last_roster = std::time::Instant::now();
            let clients = roster().await;
            let live: Vec<String> = clients.iter().map(|c| c.connection_string.clone()).collect();
            sessions.retain(|cs, s| live.contains(cs) && !s.transport.is_closed());
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
    spawn_assist_dispatcher();
    spawn_chat_bridge();
    spawn_shelf_notifier();

    if mcp_http {
        crate::plugins::run_plugin_mcp_server_http(manager).await
    } else {
        crate::plugins::run_plugin_mcp_server(manager).await
    }
}

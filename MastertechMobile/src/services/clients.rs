//! Remote-client session control for mobile, mirroring the egui admin
//! console. Reuses `displays`' `Cmd` wire type, bincode framing, and
//! `AdminTransport` (direct-TCP with WebSocket-relay fallback) so mobile
//! speaks the identical protocol to the Mastertech agent.

use database::schema::ConnectedClient;
use displays::tabs::admin_console::client_interface::{AdminTransport, SessionEvent, TransportKind};
use displays::{Cmd, ShellCommandType};

/// Auth-scoped snapshot of currently-connected clients (root sees the store,
/// technicians see their own). One-shot; the page polls it on an interval.
pub async fn fetch_connected_clients() -> Vec<ConnectedClient> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let _ = database::schema::utilities::get_connected_clients(tx).await;
    rx.try_recv().unwrap_or_default()
}

const LOG_CAP: usize = 500;

/// A live admin↔client control session driven off an `AdminTransport`.
pub struct ClientSession {
    pub connection_string: String,
    pub friendly: String,
    pub transport: AdminTransport,
    pub kind: TransportKind,
    pub connected: bool,
    pub status: String,
    pub log: Vec<String>,
}

impl ClientSession {
    /// Dial `client` (TCP when it advertises `local_ip`+`tcp_port`, else relay).
    pub fn open(client: &ConnectedClient) -> Option<Self> {
        let transport = AdminTransport::dial(client)?;
        let kind = transport.kind();
        Some(Self {
            connection_string: client.connection_string.clone(),
            friendly: client
                .friendly_name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| client.connection_string.clone()),
            transport,
            kind,
            connected: false,
            status: "Connecting…".to_string(),
            log: Vec::new(),
        })
    }

    pub fn transport_label(&self) -> &'static str {
        match self.kind {
            TransportKind::Tcp => "TCP",
            TransportKind::WebSocket => "Relay",
        }
    }

    /// Drain queued transport events into the log. Returns true if anything changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        while let Some(event) = self.transport.poll_event() {
            changed = true;
            match event {
                SessionEvent::Opened => {
                    self.connected = true;
                    self.status = "Connected".to_string();
                    self.push("● connected");
                }
                SessionEvent::Closed => {
                    self.connected = false;
                    self.status = "Disconnected".to_string();
                    self.push("○ disconnected");
                }
                SessionEvent::Error(e) => {
                    self.status = e.clone();
                    self.push(&format!("! {e}"));
                }
                SessionEvent::Text(t) => {
                    for line in t.lines() {
                        self.push(line);
                    }
                }
                SessionEvent::Cmd(cmd) => {
                    if let Some(line) = summarize_cmd(&cmd) {
                        self.push(&line);
                    }
                }
                SessionEvent::Binary(_) => {}
            }
        }
        changed
    }

    fn push(&mut self, line: &str) {
        self.log.push(line.to_string());
        if self.log.len() > LOG_CAP {
            let overflow = self.log.len() - LOG_CAP;
            self.log.drain(..overflow);
        }
    }

    pub fn run_shell(&mut self, command: String) {
        self.push(&format!("> {command}"));
        self.transport.send_cmd(&Cmd::ShellCommand {
            command,
            shell: ShellCommandType::Auto,
        });
    }

    pub fn reboot(&mut self) {
        self.push("> reboot");
        self.transport.send_cmd(&Cmd::RebootSystem {
            persist_mastertech: true,
            terminal_mode: false,
        });
    }

    pub fn shutdown(&mut self) {
        self.push("> shutdown");
        self.transport.send_cmd(&Cmd::ShutdownSystem);
    }

    pub fn lock(&mut self) {
        self.push("> lock workstation");
        self.transport.send_cmd(&Cmd::LockWorkstation);
    }

    pub fn refresh_live(&mut self) {
        self.push("> request live data");
        self.transport.send_cmd(&Cmd::LiveData);
    }

    pub fn disconnect(&mut self) {
        self.transport.close();
        self.connected = false;
        self.status = "Closed".to_string();
        self.push("○ session closed");
    }
}

/// One-line summary for the structured `Cmd` responses worth surfacing in the
/// mobile log. Streamed stdout arrives as text frames, not these.
fn summarize_cmd(cmd: &Cmd) -> Option<String> {
    match cmd {
        Cmd::ServiceActionResponse { name, success, message } => {
            Some(format!("service {name}: {} {message}", ok(*success)))
        }
        Cmd::RemoteScriptLog(s) => Some(s.clone()),
        Cmd::WindowsUpdateResult { success, summary } => {
            Some(format!("windows update: {} {summary}", ok(*success)))
        }
        Cmd::UninstallProgramResult { id, success, message } => {
            Some(format!("uninstall {id}: {} {message}", ok(*success)))
        }
        Cmd::LoadWasmPluginResult { plugin_id, success, message } => {
            Some(format!("plugin {plugin_id}: {} {message}", ok(*success)))
        }
        _ => None,
    }
}

fn ok(success: bool) -> &'static str {
    if success { "ok" } else { "failed" }
}

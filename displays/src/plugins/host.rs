use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

/// Events flowing between plugins and the host application.
///
/// Plugins emit events via `PluginHost::emit()`.
/// The host (PluginManager) drains the channel each frame and dispatches accordingly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginEvent {
    // ── Plugin → Host ──────────────────────────────────────────────────
    RequestRepaint,

    ShowNotification {
        title: String,
        body: String,
        kind: NotificationKind,
    },

    RunScript {
        client_id: String,
        filename: String,
        content: String,
    },

    SendWsCommand {
        client_id: String,
        payload: Vec<u8>,
    },

    /// Arbitrary plugin-defined event.
    Custom {
        plugin_id: String,
        event_type: String,
        data: serde_json::Value,
    },

    // ── Host → Plugin (broadcast) ──────────────────────────────────────
    ClientConnected(ClientSnapshot),
    ClientDisconnected(String),
    SystemInfoUpdated(SystemInfoSnapshot),

    ScriptCompleted {
        client_id: String,
        filename: String,
        success: bool,
        output: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationKind {
    Success,
    Error,
    Warning,
    Info,
}

/// Lightweight read-only snapshot of a connected client, safe to hand to plugins.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientSnapshot {
    pub connection_string: String,
    pub friendly_name: Option<String>,
    pub hostname: Option<String>,
    pub is_connected: bool,
}

/// Lightweight read-only snapshot of system information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemInfoSnapshot {
    pub hostname: String,
    pub cpu: String,
    pub ram_total_mb: u64,
    pub os: String,
}

/// Lightweight read-only snapshot of the current logged-in user.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserSnapshot {
    pub username: String,
    pub store_id: String,
    pub email: String,
}

/// The API surface that plugins use to communicate with the Mastertech host.
///
/// Plugins receive an immutable reference to `PluginHost` during `logic()` and `ui()` calls.
/// To emit events, plugins use the `event_tx` sender directly or the convenience helpers.
pub struct PluginHost {
    /// Send events from plugin → host.
    pub event_tx: Sender<PluginEvent>,
    /// Receive events from host → plugins (broadcast channel).
    pub broadcast_rx: Receiver<PluginEvent>,
    /// Sender side kept by the manager for broadcasting host → plugin events.
    pub(crate) broadcast_tx: Sender<PluginEvent>,

    /// Cloneable egui context for `request_repaint` from any thread.
    pub ctx: Option<eframe::egui::Context>,

    // ── Data snapshots (updated by PluginManager each frame) ───────────
    pub connected_clients: Vec<ClientSnapshot>,
    pub current_user: Option<UserSnapshot>,
    pub system_info: Option<SystemInfoSnapshot>,
}

impl PluginHost {
    pub fn new() -> Self {
        let (event_tx, event_rx) = crossbeam::channel::unbounded();
        let (broadcast_tx, broadcast_rx) = crossbeam::channel::unbounded();
        Self {
            event_tx,
            broadcast_rx,
            broadcast_tx,
            ctx: None,
            connected_clients: Vec::new(),
            current_user: None,
            system_info: None,
        }
    }

    // ── Convenience helpers ────────────────────────────────────────────

    pub fn request_repaint(&self) {
        let _ = self.event_tx.try_send(PluginEvent::RequestRepaint);
    }

    pub fn send_notification(&self, title: impl Into<String>, body: impl Into<String>, kind: NotificationKind) {
        let _ = self.event_tx.try_send(PluginEvent::ShowNotification {
            title: title.into(),
            body: body.into(),
            kind,
        });
    }

    pub fn run_remote_script(&self, client_id: impl Into<String>, filename: impl Into<String>, content: impl Into<String>) {
        let _ = self.event_tx.try_send(PluginEvent::RunScript {
            client_id: client_id.into(),
            filename: filename.into(),
            content: content.into(),
        });
    }

    pub fn emit_custom(&self, plugin_id: impl Into<String>, event_type: impl Into<String>, data: serde_json::Value) {
        let _ = self.event_tx.try_send(PluginEvent::Custom {
            plugin_id: plugin_id.into(),
            event_type: event_type.into(),
            data,
        });
    }

    /// Broadcast an event from the host to all plugins.
    pub(crate) fn broadcast(&self, event: PluginEvent) {
        let _ = self.broadcast_tx.try_send(event);
    }
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

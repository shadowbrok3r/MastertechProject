//! Per-client log of MCP tool calls served by the admin's MCP server.
//!
//! Populated by a `ServerHandler::call_tool` interceptor in
//! `mcp_bridge.rs` that fires `start_call` before dispatching to the
//! tool router and `finish_call` once the inner future resolves —
//! catching *every* tool invocation (local DB queries, remote-egui
//! injection, stress runs, the lot) rather than only the proxied
//! `call_remote_plugin_tool` path.
//!
//! Entries are keyed by `connection_string` when the tool's arguments
//! carry one (`remote_egui_*`, `call_remote_plugin_tool`, etc.), so the
//! per-client viewer in `WebSocketClient::show` filters to the calls
//! that targeted that client. Tools that don't target a specific client
//! land under [`GLOBAL_KEY`]; the viewer's `get_for_client` merges those
//! in alongside the per-client list so the operator sees the full
//! picture from any client's tab.
//!
//! Same access pattern as `open_service_suggestions`: a process-wide
//! `Mutex<HashMap>` accessed via free functions, so we don't have to
//! thread a sender through every transport layer.

/// Key under which tool calls that don't target a specific connected
/// client are stored (DB queries, plugin authoring, etc.). The viewer
/// merges these into every per-client view via [`get_for_client`].
pub const GLOBAL_KEY: &str = "_mcp_server";

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use web_time::Instant;

/// Hard cap on entries kept per client. Older completed entries fall off
/// the front when the cap is hit; in-flight (`Pending`) entries are kept
/// so the UI can still show what's currently running.
const MAX_ENTRIES_PER_CLIENT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolCallStatus {
    Pending,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct McpToolCallLog {
    pub request_id: String,
    pub plugin_id: String,
    pub tool_name: String,
    pub args_json: String,
    pub status: McpToolCallStatus,
    pub started_at: Instant,
    pub finished_at: Option<Instant>,
    pub result_json: Option<String>,
}

impl McpToolCallLog {
    pub fn elapsed_ms(&self) -> u128 {
        self.finished_at.unwrap_or_else(Instant::now)
            .saturating_duration_since(self.started_at)
            .as_millis()
    }
}

struct StoreInner {
    by_client: HashMap<String, VecDeque<McpToolCallLog>>,
    request_to_client: HashMap<String, String>,
}

static STORE: OnceLock<Mutex<StoreInner>> = OnceLock::new();

fn store() -> &'static Mutex<StoreInner> {
    STORE.get_or_init(|| {
        Mutex::new(StoreInner {
            by_client: HashMap::new(),
            request_to_client: HashMap::new(),
        })
    })
}

/// Record a new tool call starting. Called from `mcp_bridge` right
/// after the request_id is generated and before the bytes go out on
/// the wire.
pub fn start_call(
    connection_string: &str,
    request_id: String,
    plugin_id: String,
    tool_name: String,
    args_json: String,
) {
    let Ok(mut g) = store().lock() else { return };
    g.request_to_client
        .insert(request_id.clone(), connection_string.to_string());
    let entry = McpToolCallLog {
        request_id,
        plugin_id,
        tool_name,
        args_json,
        status: McpToolCallStatus::Pending,
        started_at: Instant::now(),
        finished_at: None,
        result_json: None,
    };
    let list = g
        .by_client
        .entry(connection_string.to_string())
        .or_default();
    list.push_back(entry);
    evict_overflow(list);
}

/// Mark a call as finished. Looks up the client by `request_id` so
/// callers that don't have the connection_string handy (e.g. timeout
/// guards) can still terminate the entry.
pub fn finish_call(request_id: &str, success: bool, result_json: String) {
    let Ok(mut g) = store().lock() else { return };
    let Some(cs) = g.request_to_client.remove(request_id) else {
        return;
    };
    if let Some(list) = g.by_client.get_mut(&cs) {
        if let Some(e) = list.iter_mut().find(|e| e.request_id == request_id) {
            e.status = if success {
                McpToolCallStatus::Success
            } else {
                McpToolCallStatus::Error
            };
            e.finished_at = Some(Instant::now());
            e.result_json = Some(result_json);
        }
    }
}

/// Force-terminate a still-pending entry, used on RAII drop paths when
/// `call_remote_plugin_tool` times out or its response channel closes.
/// No-op if the entry already finished normally.
pub fn mark_aborted(request_id: &str, reason: &str) {
    let Ok(mut g) = store().lock() else { return };
    let Some(cs) = g.request_to_client.remove(request_id) else {
        return;
    };
    if let Some(list) = g.by_client.get_mut(&cs) {
        if let Some(e) = list.iter_mut().find(|e| e.request_id == request_id) {
            if matches!(e.status, McpToolCallStatus::Pending) {
                e.status = McpToolCallStatus::Error;
                e.finished_at = Some(Instant::now());
                e.result_json = Some(format!("{{\"aborted\":\"{reason}\"}}"));
            }
        }
    }
}

/// Snapshot of all entries for a client, oldest first.
pub fn get(connection_string: &str) -> Vec<McpToolCallLog> {
    store()
        .lock()
        .ok()
        .and_then(|g| g.by_client.get(connection_string).cloned())
        .map(|d| d.into_iter().collect())
        .unwrap_or_default()
}

/// Per-client view that also folds in the global / non-client-targeted
/// calls (DB queries, list_plugins, etc.). Sorted by `started_at` so
/// the merged feed reads chronologically.
pub fn get_for_client(connection_string: &str) -> Vec<McpToolCallLog> {
    let mut entries = get(connection_string);
    if connection_string != GLOBAL_KEY {
        entries.extend(get(GLOBAL_KEY));
        entries.sort_by_key(|e| e.started_at);
    }
    entries
}

pub fn pending_count(connection_string: &str) -> usize {
    let Ok(g) = store().lock() else { return 0 };
    let count = |key: &str| -> usize {
        g.by_client
            .get(key)
            .map(|d| d.iter().filter(|e| matches!(e.status, McpToolCallStatus::Pending)).count())
            .unwrap_or(0)
    };
    let mut n = count(connection_string);
    if connection_string != GLOBAL_KEY {
        n += count(GLOBAL_KEY);
    }
    n
}

pub fn clear(connection_string: &str) {
    let Ok(mut g) = store().lock() else { return };
    if let Some(list) = g.by_client.get_mut(connection_string) {
        list.retain(|e| matches!(e.status, McpToolCallStatus::Pending));
    }
}

fn evict_overflow(list: &mut VecDeque<McpToolCallLog>) {
    while list.len() > MAX_ENTRIES_PER_CLIENT {
        let evict_idx = list
            .iter()
            .position(|e| !matches!(e.status, McpToolCallStatus::Pending));
        match evict_idx {
            Some(i) => {
                list.remove(i);
            }
            None => break,
        }
    }
}

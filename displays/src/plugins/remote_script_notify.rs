//! Remote script log/result accumulation for the admin console receive path.
//!
//! Split out of `mcp_bridge` so WASM builds compile `displays` without `rmcp` / `tokio` MCP.
//! Desktop `mcp_bridge::scripts_run_remote` shares `REMOTE_SCRIPT_PENDING` + `REMOTE_SCRIPT_ACCUM`.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

/// Accumulated in-flight log/result data for the active remote-script session.
#[derive(Debug, Default)]
pub struct RemoteScriptSession {
    pub session_id: String,
    pub logs: Vec<String>,
    pub results: Vec<(String, String)>, // (name, status)
    pub complete: bool,
}

/// Per-client in-flight log/result accumulators, keyed by connection_string.
pub static REMOTE_SCRIPT_ACCUM: Lazy<Mutex<HashMap<String, RemoteScriptSession>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Per-client MCP waiters keyed by connection_string; value is (script_name, sender).
#[cfg(not(target_arch = "wasm32"))]
pub static REMOTE_SCRIPT_PENDING: Lazy<
    Mutex<HashMap<String, (String, tokio::sync::oneshot::Sender<RemoteScriptSession>)>>,
> = Lazy::new(|| Mutex::new(HashMap::new()));

/// One-shot waiters fulfilled (with the category count) by the next `RemoteScriptListResponse`.
#[cfg(not(target_arch = "wasm32"))]
pub static SCRIPT_LIST_WAITERS: Lazy<Mutex<Vec<tokio::sync::oneshot::Sender<usize>>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

#[cfg(not(target_arch = "wasm32"))]
pub fn register_script_list_waiter() -> tokio::sync::oneshot::Receiver<usize> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Ok(mut g) = SCRIPT_LIST_WAITERS.lock() {
        g.push(tx);
    }
    rx
}

/// Called by the admin console's receive handler when a `RemoteScriptListResponse` arrives.
pub fn notify_script_list(category_count: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(mut g) = SCRIPT_LIST_WAITERS.lock() {
            for tx in g.drain(..) {
                let _ = tx.send(category_count);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = category_count;
    }
}

/// Pending deploy acks keyed by plugin_id; fulfilled by `LoadWasmPluginResult`.
#[cfg(not(target_arch = "wasm32"))]
pub static DEPLOY_ACK_PENDING: Lazy<
    Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<(bool, String)>>>,
> = Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

/// Called by the admin console's receive handler when a `LoadWasmPluginResult` arrives.
pub fn notify_deploy_ack(plugin_id: &str, success: bool, message: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(mut guard) = DEPLOY_ACK_PENDING.lock() {
            if let Some(tx) = guard.remove(plugin_id) {
                let _ = tx.send((success, message.to_string()));
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (plugin_id, success, message);
    }
}

/// Log lines retained per in-flight session; the oldest are dropped past this.
const MAX_ACCUM_LOGS: usize = 2_000;
/// Results retained per in-flight session.
const MAX_ACCUM_RESULTS: usize = 2_000;

/// Called by the admin console's receive handler when a `RemoteScriptLog` message arrives from a client.
pub fn notify_remote_script_log(conn: &str, msg: String) {
    if let Ok(mut accum) = REMOTE_SCRIPT_ACCUM.lock() {
        let logs = &mut accum.entry(conn.to_string()).or_default().logs;
        logs.push(msg);
        if logs.len() > MAX_ACCUM_LOGS {
            let excess = logs.len() - MAX_ACCUM_LOGS;
            logs.drain(..excess);
        }
    }
}

/// Called by the admin console's receive handler when a `RemoteScriptResult` message arrives.
pub fn notify_remote_script_result(conn: &str, name: String, status: String) {
    if let Ok(mut accum) = REMOTE_SCRIPT_ACCUM.lock() {
        let results = &mut accum.entry(conn.to_string()).or_default().results;
        results.push((name, status));
        if results.len() > MAX_ACCUM_RESULTS {
            let excess = results.len() - MAX_ACCUM_RESULTS;
            results.drain(..excess);
        }
    }
}

/// Drop a client's accumulator when its admin session tears down. Dropping any
/// pending sender makes its MCP waiter fail fast instead of waiting for a timeout.
pub fn drop_session(conn: &str) {
    if let Ok(mut accum) = REMOTE_SCRIPT_ACCUM.lock() {
        accum.remove(conn);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(mut guard) = REMOTE_SCRIPT_PENDING.lock() {
        guard.remove(conn);
    }
}

/// Called by the admin console's receive handler when a `RemoteScriptsComplete` message arrives.
pub fn notify_remote_scripts_complete(conn: &str) {
    let session = {
        let mut accum = match REMOTE_SCRIPT_ACCUM.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match accum.remove(conn) {
            Some(mut out) => {
                out.complete = true;
                out
            }
            None => return,
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(mut guard) = REMOTE_SCRIPT_PENDING.lock() {
            // Deliver only when this client's awaited script appears in the results.
            let matches_pending = guard
                .get(conn)
                .map(|(name, _)| session.results.iter().any(|(n, _)| n == name))
                .unwrap_or(false);
            if matches_pending {
                if let Some((_, tx)) = guard.remove(conn) {
                    let _ = tx.send(session);
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(session);
    }
}

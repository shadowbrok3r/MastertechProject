//! Remote script log/result accumulation for the admin console receive path.
//!
//! Split out of `mcp_bridge` so WASM builds compile `displays` without `rmcp` / `tokio` MCP.
//! Desktop `mcp_bridge::scripts_run_remote` shares `REMOTE_SCRIPT_PENDING` + `REMOTE_SCRIPT_ACCUM`.

use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Accumulated in-flight log/result data for the active remote-script session.
#[derive(Debug, Default)]
pub struct RemoteScriptSession {
    pub session_id: String,
    pub logs: Vec<String>,
    pub results: Vec<(String, String)>, // (name, status)
    pub complete: bool,
}

pub static REMOTE_SCRIPT_ACCUM: Lazy<Mutex<RemoteScriptSession>> =
    Lazy::new(|| Mutex::new(RemoteScriptSession::default()));

#[cfg(not(target_arch = "wasm32"))]
pub static REMOTE_SCRIPT_PENDING: Lazy<
    Mutex<
        Option<(
            String,
            tokio::sync::oneshot::Sender<RemoteScriptSession>,
        )>,
    >,
> = Lazy::new(|| Mutex::new(None));

/// Called by the admin console's receive handler when a `RemoteScriptLog` message arrives from a client.
pub fn notify_remote_script_log(msg: String) {
    if let Ok(mut accum) = REMOTE_SCRIPT_ACCUM.lock() {
        accum.logs.push(msg);
    }
}

/// Called by the admin console's receive handler when a `RemoteScriptResult` message arrives.
pub fn notify_remote_script_result(name: String, status: String) {
    if let Ok(mut accum) = REMOTE_SCRIPT_ACCUM.lock() {
        accum.results.push((name, status));
    }
}

/// Called by the admin console's receive handler when a `RemoteScriptsComplete` message arrives.
pub fn notify_remote_scripts_complete() {
    let session = {
        let mut accum = match REMOTE_SCRIPT_ACCUM.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut out = RemoteScriptSession::default();
        std::mem::swap(&mut *accum, &mut out);
        out.complete = true;
        out
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(mut guard) = REMOTE_SCRIPT_PENDING.lock() {
            if let Some((_, tx)) = guard.take() {
                let _ = tx.send(session);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(session);
    }
}

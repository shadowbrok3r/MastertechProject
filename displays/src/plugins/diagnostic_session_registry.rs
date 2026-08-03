//! Open diagnostic_session id per Web Console connection_string for MCP auto-linking.
//!
//! The map is process-local, so it is lost on app restart and never populated
//! for a session opened elsewhere. `resolve_open_session` therefore falls back
//! to the `diagnostic_session` table before reporting no session.

use database::schema::{DiagnosticSession, RecordIdExt};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

static ACTIVE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn normalize_session_key(session_id: &str) -> String {
    session_id
        .trim()
        .strip_prefix("diagnostic_session:")
        .unwrap_or(session_id.trim())
        .to_string()
}

/// Remember the open session for a connected client.
pub fn register(connection_string: &str, session_id: &str) {
    let cs = connection_string.trim();
    let sid = normalize_session_key(session_id);
    if cs.is_empty() || sid.is_empty() {
        return;
    }
    if let Ok(mut map) = ACTIVE.lock() {
        map.insert(cs.to_string(), sid);
    }
}

/// Lookup the open session for a connection_string.
pub fn get(connection_string: &str) -> Option<String> {
    ACTIVE
        .lock()
        .ok()?
        .get(connection_string.trim())
        .cloned()
}

/// Open session for a connection: the in-memory pin when it still names an
/// open row, else the newest open `diagnostic_session` for the connection (or
/// for the computer its `connected_client` points at). A database hit re-pins
/// the registry. Returns `None` only when no open session exists.
pub async fn resolve_open_session(connection_string: &str) -> Option<DiagnosticSession> {
    let cs = connection_string.trim();
    if cs.is_empty() {
        return None;
    }
    if let Some(sid) = get(cs) {
        match DiagnosticSession::get(&sid).await {
            Ok(Some(s)) if s.status == "open" => return Some(s),
            // Pin names a closed or deleted row — drop it and re-resolve.
            Ok(_) => clear_connection(cs),
            Err(e) => log::warn!("session registry: pinned session {sid} unreadable: {e}"),
        }
    }
    let computer = connected_client_computer(cs).await;
    let session = match DiagnosticSession::latest_open_for_connection(cs, computer.as_ref()).await {
        Ok(found) => found,
        Err(e) => {
            log::warn!("session registry: open-session lookup failed for {cs}: {e}");
            None
        }
    };
    if let Some(s) = &session {
        register(cs, &s.id.key_string());
    }
    session
}

/// Session key form of [`resolve_open_session`].
pub async fn resolve_open_session_key(connection_string: &str) -> Option<String> {
    resolve_open_session(connection_string)
        .await
        .map(|s| s.id.key_string())
}

/// The computer a connected client points at, for the by-computer session fallback.
async fn connected_client_computer(
    connection_string: &str,
) -> Option<database::schema::RecordId> {
    database::db()
        .query(
            "SELECT VALUE computer FROM connected_client \
             WHERE connection_string == $cs LIMIT 1",
        )
        .bind(("cs", connection_string.to_string()))
        .await
        .and_then(|mut r| r.take::<Vec<Option<database::schema::RecordId>>>(0))
        .map(|v| v.into_iter().flatten().next())
        .unwrap_or(None)
}

/// The sole active connection_string, when exactly one session is registered.
/// Used to default the link target for a local dump analysis.
pub fn single_active_connection() -> Option<String> {
    let map = ACTIVE.lock().ok()?;
    if map.len() == 1 {
        map.keys().next().cloned()
    } else {
        None
    }
}

/// Drop the registry entry for a connection_string.
pub fn clear_connection(connection_string: &str) {
    if let Ok(mut map) = ACTIVE.lock() {
        map.remove(connection_string.trim());
    }
}

/// Drop any registry entry whose session id matches (full or bare key).
pub fn clear_session(session_id: &str) {
    let needle = normalize_session_key(session_id);
    if needle.is_empty() {
        return;
    }
    if let Ok(mut map) = ACTIVE.lock() {
        map.retain(|_, v| normalize_session_key(v) != needle);
    }
}

//! Open diagnostic_session id per Web Console connection_string for MCP auto-linking.
//!
//! The map only holds sessions created since this process started, so without a
//! database fallback an app restart or client reconnect silently drops
//! `session_ref` and `task_ref` from every subsequent record. `resolve_open_session`
//! reads the typed row; `resolve_open_session_key` degrades to an id-only
//! projection so linkage still resolves on a row that will not deserialize.

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

/// Open session for a connection as a typed row: the in-memory pin when it
/// still names an open row, else the newest open `diagnostic_session` for the
/// connection (or for the computer its `connected_client` points at). A
/// database hit re-pins the registry.
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

/// Session key for a connection, for callers that only need the link target.
/// Falls back to an id-only projection when the typed row will not deserialize.
pub async fn resolve_open_session_key(connection_string: &str) -> Option<String> {
    if let Some(session) = resolve_open_session(connection_string).await {
        return Some(session.id.key_string());
    }
    projected_open_session_key(connection_string.trim()).await
}

/// Newest open session key for a connection, deserializing no field but the id.
/// `started_at` stays in the projection — ORDER BY only accepts selected idioms.
async fn projected_open_session_key(cs: &str) -> Option<String> {
    if cs.is_empty() {
        return None;
    }
    let rows: Vec<serde_json::Value> = database::db()
        .query(
            "SELECT record::id(id) AS session_key, started_at FROM diagnostic_session \
             WHERE status == 'open' AND connection_string == $cs \
             ORDER BY started_at DESC LIMIT 1",
        )
        .bind(("cs", cs.to_string()))
        .await
        .ok()?
        .take(0)
        .ok()?;
    let sid = normalize_session_key(rows.first()?.get("session_key")?.as_str()?);
    if sid.is_empty() {
        return None;
    }
    register(cs, &sid);
    Some(sid)
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

//! Open diagnostic_session id per Web Console connection_string for MCP auto-linking.

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

/// Lookup the open session for a connection_string, falling back to the
/// newest open `diagnostic_session` row in the database on a registry miss.
/// The map only holds sessions created since this process started, so
/// without the fallback an admin restart silently drops `session_ref` and
/// `task_ref` from every subsequent stress run.
pub async fn get_or_lookup(connection_string: &str) -> Option<String> {
    let cs = connection_string.trim();
    if cs.is_empty() {
        return None;
    }
    if let Some(hit) = get(cs) {
        return Some(hit);
    }
    // Projects the key rather than deserializing DiagnosticSession, so the
    // lookup still resolves on rows where an unrelated required field is NONE.
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

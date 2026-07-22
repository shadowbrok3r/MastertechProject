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

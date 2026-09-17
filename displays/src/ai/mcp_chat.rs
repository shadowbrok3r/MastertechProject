#![cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
//! The per-machine agent config file (`<APPDATA|HOME>/MasterTech/zeroclaw.json`)
//! and the standing-session hosts the admin console reads from it.

/// Per-machine override of the compiled-in gateway.
pub fn zeroclaw_config_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    std::path::Path::new(&base).join("MasterTech").join("zeroclaw.json")
}

/// Hostnames the BSOD autopilot needs a standing admin session to, from
/// `autopilot_hosts` in the gateway config file. Remote MCP tools fail without
/// a live session, so the console must hold one open for each.
pub fn autopilot_hosts() -> Vec<String> {
    // ensure_sessions runs every frame; re-read at most every 30s.
    static CACHE: std::sync::OnceLock<std::sync::Mutex<(std::time::Instant, Vec<String>)>> =
        std::sync::OnceLock::new();
    let cell = CACHE.get_or_init(|| {
        std::sync::Mutex::new((
            std::time::Instant::now() - std::time::Duration::from_secs(3600),
            Vec::new(),
        ))
    });
    if let Ok(mut guard) = cell.lock() {
        if guard.0.elapsed() < std::time::Duration::from_secs(30) {
            return guard.1.clone();
        }
        let fresh = read_autopilot_hosts();
        *guard = (std::time::Instant::now(), fresh.clone());
        return fresh;
    }
    read_autopilot_hosts()
}

fn read_autopilot_hosts() -> Vec<String> {
    std::fs::read_to_string(zeroclaw_config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v["autopilot_hosts"].as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

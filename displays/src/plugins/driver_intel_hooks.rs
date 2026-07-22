//! Persist driverstore plugin snapshots and match them against the fleet blocklist.
//!
//! `WebSocketClient::receive` routes `com.mastertech.driverstore::snapshot`
//! results here. The pnputil text is parsed into a `driver_snapshot` row,
//! diffed against the machine's previous snapshot, and matched against
//! `known_bad_driver`; hits surface as toasts, notices, and session findings.

use std::collections::HashMap;
use std::sync::Mutex;

use database::schema::{
    driver_intel::{diff_driver_sets, parse_pnputil_enum, DriverSnapshot, KnownBadDriver},
    DiagnosticCategory, DiagnosticEntry, PluginUsageRef, RecordId, RecordIdExt,
    DIAGNOSTIC_SESSION_TABLE,
};
use once_cell::sync::Lazy;

use crate::{get_toast_sender, PlatformSpawner, Spawner, ToastMessage};

pub const DRIVERSTORE_PLUGIN_ID: &str = "com.mastertech.driverstore";

/// Label to stamp on the next snapshot per connection_string ('intake', 'pre_service', …).
static PENDING_LABELS: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Stamp the next snapshot from this client with a label.
pub fn set_pending_label(connection_string: &str, label: &str) {
    if let Ok(mut map) = PENDING_LABELS.lock() {
        map.insert(connection_string.to_string(), label.to_string());
    }
}

fn take_pending_label(connection_string: &str) -> String {
    PENDING_LABELS
        .lock()
        .ok()
        .and_then(|mut map| map.remove(connection_string))
        .unwrap_or_else(|| "manual".to_string())
}

/// Drop a pending label without consuming it into a snapshot. Called by
/// driver_snapshot_take on error paths where the plugin result never reaches
/// ingest, so a stranded label can't mislabel the next snapshot.
pub fn clear_pending_label(connection_string: &str) {
    if let Ok(mut map) = PENDING_LABELS.lock() {
        map.remove(connection_string);
    }
}

/// True for driverstore results that carry a full inventory.
pub fn is_driver_snapshot_result(plugin_id: &str, tool_name: &str) -> bool {
    plugin_id == DRIVERSTORE_PLUGIN_ID && tool_name == "snapshot"
}

async fn log_finding(session_key: &str, title: String, detail: String, data: serde_json::Value) {
    let entry = DiagnosticEntry {
        session_ref: RecordId::new(DIAGNOSTIC_SESSION_TABLE, session_key),
        category: DiagnosticCategory::Finding,
        title,
        detail,
        data: Some(data),
        plugins_used: vec![PluginUsageRef {
            plugin_id: DRIVERSTORE_PLUGIN_ID.to_string(),
            tool_name: "snapshot".to_string(),
        }],
        ..Default::default()
    };
    if let Err(e) = DiagnosticEntry::create(&entry).await {
        log::warn!("driver_intel: failed to log diagnostic entry: {e}");
    }
}

/// Parse a driverstore `snapshot` payload, persist it, diff it, and blocklist-match it.
pub fn ingest_driver_snapshot(
    connection_string: String,
    computer: Option<RecordId>,
    result_json: String,
) {
    // Consume the pending label up front so a bad/empty payload can't strand it
    // for the next snapshot on this connection.
    let label = take_pending_label(&connection_string);
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&result_json) else {
        return;
    };
    let Some(text) = payload.get("driver_text").and_then(|v| v.as_str()) else {
        return;
    };
    let drivers = parse_pnputil_enum(text);
    if drivers.is_empty() {
        return;
    }

    PlatformSpawner::spawn(async move {
        let session_key = super::diagnostic_session_registry::get(&connection_string);
        let previous = DriverSnapshot::list_for_connection(&connection_string, 1)
            .await
            .ok()
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });

        let snapshot = DriverSnapshot {
            id: database::schema::random_record_id(database::schema::DRIVER_SNAPSHOT_TABLE),
            connection_string: connection_string.clone(),
            computer,
            session_ref: session_key
                .as_deref()
                .map(|k| RecordId::new(DIAGNOSTIC_SESSION_TABLE, k)),
            label: label.clone(),
            source: "pnputil".to_string(),
            taken_at: chrono::Utc::now().into(),
            driver_count: drivers.len() as u32,
            drivers: drivers.clone(),
            notes: String::new(),
        };
        let snapshot_id = match DriverSnapshot::create(&snapshot).await {
            Ok(id) => id,
            Err(e) => {
                log::warn!("driver_intel: snapshot persist failed: {e}");
                return;
            }
        };

        let mut notices = vec![format!(
            "Driver snapshot '{label}' recorded: {} packages ({})",
            drivers.len(),
            snapshot_id.key_string()
        )];

        if let Some(prev) = &previous {
            let diff = diff_driver_sets(&prev.drivers, &drivers);
            if !diff.is_empty() {
                let changed: Vec<String> = diff
                    .changed
                    .iter()
                    .map(|c| format!("{} {} -> {}", c.key, c.old_version, c.new_version))
                    .collect();
                notices.push(format!(
                    "Driver drift since '{}' ({}): +{} added, -{} removed, {} version change(s){}",
                    prev.label,
                    prev.taken_at,
                    diff.added.len(),
                    diff.removed.len(),
                    diff.changed.len(),
                    if changed.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", changed.join(", "))
                    }
                ));
            }
        }

        let blocklist = KnownBadDriver::active().await.unwrap_or_default();
        let hits = KnownBadDriver::match_inventory(&blocklist, &drivers);
        for hit in &hits {
            let msg = format!(
                "KNOWN-BAD DRIVER: {} {} ({}) — {}. Fix: {}",
                hit.driver.key(),
                hit.driver.driver_version,
                hit.entry.severity,
                hit.entry.symptom,
                if hit.entry.fix.is_empty() { "see blocklist entry" } else { &hit.entry.fix }
            );
            let _ = get_toast_sender().try_send(ToastMessage::Warning(msg.clone()));
            notices.push(msg.clone());
            if let Some(key) = session_key.as_deref() {
                log_finding(
                    key,
                    format!("Known-bad driver {}", hit.driver.key()),
                    msg,
                    serde_json::to_value(hit).unwrap_or_default(),
                )
                .await;
            }
        }

        for notice in notices {
            super::crash_intel_hooks::push_shared_notice(&connection_string, notice);
        }
    });
}

//! Posts a "Seen before" note on a newly connected machine's ticket and alerts its assignee.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use database::schema::{assistant, fleet_intel, RecordId};

/// A machine is not re-checked inside this window.
const RECHECK_AFTER: Duration = Duration::from_secs(6 * 60 * 60);

static LAST_CHECK: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// `MTECH_FLEET_NOTICE=0` turns the notice off.
fn enabled() -> bool {
    std::env::var("MTECH_FLEET_NOTICE").map(|v| v.trim() != "0").unwrap_or(true)
}

/// True at most once per `RECHECK_AFTER` for a machine.
fn due(connection_string: &str) -> bool {
    let Ok(mut last) = LAST_CHECK.lock() else { return false };
    let now = Instant::now();
    match last.get(connection_string) {
        Some(at) if now.duration_since(*at) < RECHECK_AFTER => false,
        _ => {
            last.insert(connection_string.to_string(), now);
            true
        }
    }
}

/// Checks the fleet for a connecting machine; silent unless a strong match is new to its ticket.
pub async fn notice_for(connection_string: &str, computer: Option<&RecordId>) {
    let Some(computer) = computer else { return };
    if !enabled() || !due(connection_string) {
        return;
    }
    if let Err(e) = notice(connection_string, computer).await {
        log::warn!("fleet notice: {connection_string}: {e}");
    }
}

async fn notice(connection_string: &str, computer: &RecordId) -> anyhow::Result<()> {
    if fleet_intel::is_internal(computer).await? {
        return Ok(());
    }
    let Some(ticket) = fleet_intel::open_ticket(computer).await? else {
        return Ok(());
    };
    let strong = fleet_intel::fleet_pattern_check(connection_string, fleet_intel::DEFAULT_MAX_CASES)
        .await?
        .strong();
    let existing = fleet_intel::current_fleet_note(&ticket.task).await?;
    let Some(note) = fleet_intel::note_to_post(&strong, existing.as_deref()) else {
        return Ok(());
    };
    fleet_intel::post_fleet_note(&ticket.task, &ticket.service_number, &ticket.assignee, &note).await?;
    assistant::notify(
        &ticket.assignee,
        fleet_intel::TYPE_FLEET_PATTERN,
        &strong.headline(&ticket.task_name),
        None,
        Some(&ticket.task),
    )
    .await?;
    log::info!("fleet notice: {connection_string} -> {}", ticket.task_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_once_per_window() {
        let cs = "FLEET-NOTICE-TEST:1";
        assert!(due(cs));
        assert!(!due(cs));
    }
}

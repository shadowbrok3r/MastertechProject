//! Per-machine persistence of in-progress checklist state (ported from
//! QCWizard `ChecklistStore`). Lets a tech check items off, reboot the bench
//! many times, and resume — keyed by order id AND machine id so a shared/
//! recovery folder can't cross machines. Cleared once the order is signed off.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use database::orders::ChecklistState;

#[derive(Serialize, Deserialize)]
struct Envelope {
    id_order: String,
    machine_id: String,
    saved_utc: String,
    signed_off: bool,
    summary: String,
    checklist: ChecklistState,
}

/// A restored worksheet for this (order, machine).
pub struct Restored {
    pub checklist: ChecklistState,
    pub signed_off: bool,
    pub summary: String,
}

fn default_path() -> PathBuf {
    directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("qc_checklist.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("mastertech_qc_checklist.json"))
}

pub fn save(id_order: &str, machine_id: &str, checklist: &ChecklistState, signed_off: bool, summary: &str) {
    save_to(&default_path(), id_order, machine_id, checklist, signed_off, summary);
}

pub fn restore(id_order: &str, machine_id: &str) -> Option<Restored> {
    restore_from(&default_path(), id_order, machine_id)
}

pub fn clear(id_order: &str, machine_id: &str) {
    clear_at(&default_path(), id_order, machine_id);
}

pub fn save_to(path: &std::path::Path, id_order: &str, machine_id: &str, checklist: &ChecklistState, signed_off: bool, summary: &str) {
    if id_order.is_empty() {
        return; // don't persist exploration/training (no order)
    }
    let env = Envelope {
        id_order: id_order.to_string(),
        machine_id: machine_id.to_string(),
        saved_utc: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        signed_off,
        summary: summary.to_string(),
        checklist: checklist.clone(),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string(&env) {
        Ok(json) => {
            if let Err(e) = fs::write(path, json) {
                log::warn!("checklist save failed: {e}");
            }
        }
        Err(e) => log::warn!("checklist serialize failed: {e}"),
    }
}

/// Returns the saved worksheet only when both order id and machine id match.
pub fn restore_from(path: &std::path::Path, id_order: &str, machine_id: &str) -> Option<Restored> {
    if id_order.is_empty() {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    let env: Envelope = serde_json::from_str(&raw).ok()?;
    if env.id_order != id_order || env.machine_id != machine_id {
        return None; // different machine/order
    }
    Some(Restored { checklist: env.checklist, signed_off: env.signed_off, summary: env.summary })
}

/// Deletes the worksheet only if it belongs to this (order, machine).
pub fn clear_at(path: &std::path::Path, id_order: &str, machine_id: &str) {
    let Ok(raw) = fs::read_to_string(path) else { return };
    if let Ok(env) = serde_json::from_str::<Envelope>(&raw) {
        if env.id_order != id_order || env.machine_id != machine_id {
            return;
        }
    }
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::orders::checklist::ChecklistKind;
    use database::orders::ItemStatus;

    fn tmp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("qc_checklist_test_{name}_{}.json", std::process::id()));
        let _ = fs::remove_file(&p);
        p
    }

    #[test]
    fn save_restore_round_trip() {
        let path = tmp("roundtrip");
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.set_status("hw_cpu", ItemStatus::Pass);
        save_to(&path, "1022", "machineA", &c, false, "");
        let r = restore_from(&path, "1022", "machineA").expect("should restore");
        assert!(!r.signed_off);
        assert_eq!(r.checklist.item("hw_cpu").unwrap().status(), ItemStatus::Pass);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn restore_rejects_other_machine_or_order() {
        let path = tmp("isolation");
        let c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        save_to(&path, "1022", "machineA", &c, false, "");
        assert!(restore_from(&path, "1022", "machineB").is_none(), "different machine");
        assert!(restore_from(&path, "9999", "machineA").is_none(), "different order");
        assert!(restore_from(&path, "1022", "machineA").is_some());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn clear_only_own_record() {
        let path = tmp("clear");
        let c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        save_to(&path, "1022", "machineA", &c, true, "PASS");
        clear_at(&path, "1022", "machineB"); // not ours
        assert!(path.exists(), "must not delete another machine's worksheet");
        clear_at(&path, "1022", "machineA");
        assert!(!path.exists());
    }
}

//! Section-based QC checklist (ported from QCWizard `ChecklistDefinitions` /
//! `ChecklistSection`). Pure model + content: definitions, runtime state,
//! gating, and the failure rollup. No egui, no WMI — auto-verify (SMART/OA3/
//! temps) and resume I/O live in qc-app behind the model's `apply_auto` /
//! `reset_for_recheck` / `restore_from` hooks.

use serde::{Deserialize, Serialize};

use crate::SurrealValue;

// ─── enums (stored as strings to keep SurrealValue serialization trivial) ────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecklistKind {
    BuildQc,
    Repair,
}

impl ChecklistKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BuildQc => "BuildQC",
            Self::Repair => "Repair",
        }
    }

    pub fn for_order_kind(kind: super::OrderKind) -> Self {
        match kind {
            super::OrderKind::Repair | super::OrderKind::Service => Self::Repair,
            _ => Self::BuildQc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Unset,
    Pass,
    Fail,
    Na,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unset => "Unset",
            Self::Pass => "Pass",
            Self::Fail => "Fail",
            Self::Na => "NA",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "Pass" => Self::Pass,
            "Fail" => Self::Fail,
            "NA" => Self::Na,
            _ => Self::Unset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckSource {
    None,
    Auto,
    Manual,
}

impl CheckSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Auto => "Auto",
            Self::Manual => "Manual",
        }
    }
}

// ─── static definitions ──────────────────────────────────────────────────────

pub struct ItemDef {
    pub key: &'static str,
    pub text: &'static str,
    pub captures_value: bool,
}

pub struct SectionDef {
    pub number: i64,
    pub title: &'static str,
    pub notes: &'static str,
    pub items: &'static [ItemDef],
}

pub struct ChecklistDef {
    pub kind: ChecklistKind,
    pub sections: &'static [SectionDef],
}

const fn item(key: &'static str, text: &'static str, captures_value: bool) -> ItemDef {
    ItemDef { key, text, captures_value }
}

pub fn def_for(kind: ChecklistKind) -> &'static ChecklistDef {
    match kind {
        ChecklistKind::BuildQc => &BUILD_QC,
        ChecklistKind::Repair => &REPAIR,
    }
}

/// Build/QC content — Shane's final Xidax QC checklist (Asana 2026-06-16).
/// Item keys are stable; auto-verify and failure rows hang off them.
pub static BUILD_QC: ChecklistDef = ChecklistDef {
    kind: ChecklistKind::BuildQc,
    sections: &[
        SectionDef { number: 1, title: "Hardware / Component Inspection", notes: "", items: &[
            item("hw_cpu", "CPU model verified against build order", false),
            item("hw_gpu", "GPU model verified against build order", false),
            item("hw_ram", "RAM capacity and speed verified against build order", false),
            item("hw_storage", "Storage drives (type and capacity) verified against build order", false),
            item("hw_pcie_seated", "All PCIe components fully seated", false),
            item("hw_ram_slots", "RAM in correct slots per motherboard spec (A2/B2 for two sticks)", false),
            item("hw_cooler_mounted", "CPU cooler properly mounted and secured", false),
            item("hw_power_connectors", "All power connectors attached (24-pin, CPU/EPS, every PCIe/GPU 8-pin)", false),
            item("hw_no_damage", "No bent pins, physical damage, or missing hardware", false),
            item("hw_thermal_paste", "Thermal paste applied correctly", false),
            item("hw_serials", "Serial numbers recorded to the order: CPU, GPU, motherboard, PSU, each drive", true),
        ]},
        SectionDef { number: 2, title: "Cable Management", notes: "", items: &[
            item("cab_channels", "All cables routed through cable management channels", false),
            item("cab_airflow", "No cables crossing or obstructing fans / airflow", false),
            item("cab_secured", "Cables secured with ties or velcro straps", false),
            item("cab_no_loose", "No loose or hanging cables inside chassis", false),
            item("cab_psu_back", "PSU cables organized in back panel area", false),
            item("cab_front_panel", "Front panel connectors neatly managed", false),
        ]},
        SectionDef { number: 3, title: "Liquid Cooling — AIO & Custom Loop (skip if air-cooled)", notes: "", items: &[
            item("lc_leak_test", "Custom loop pressure/leak-tested before fill, then 24-hour leak watch", false),
            item("lc_pump_rpm", "Pump powered and RPM detected in BIOS/software", false),
            item("lc_coolant_level", "Coolant level correct, no air pocket starving the pump", false),
            item("lc_tubing", "Tubing secure at every fitting, no kinks, not rubbing a fan", false),
            item("lc_radiator", "Radiator mounted solid, fans oriented correctly (intake/exhaust per build)", false),
            item("lc_coldplate", "AIO cold plate seated flat, no rocking", false),
            item("lc_no_weeping", "No coolant residue or weeping anywhere after the leak watch", false),
        ]},
        SectionDef { number: 4, title: "BIOS / Firmware", notes: "", items: &[
            item("bios_updated", "BIOS updated to latest stable version", false),
            item("bios_xmp", "XMP / EXPO memory profile enabled (if applicable)", false),
            item("bios_clocks", "CPU clocks and core temps verified in BIOS", false),
            item("bios_boot_order", "Boot order correct — OS drive first", false),
            item("bios_secure_boot", "Secure Boot configured per build spec", false),
            item("bios_fan_curves", "Fan curves configured and verified", false),
            item("bios_post", "System POSTs with no errors", false),
            item("bios_datetime", "System date and time correct", false),
        ]},
        SectionDef { number: 5, title: "OS & Driver Installation", notes: "", items: &[
            item("os_activated", "Windows fully installed and product key activated", false),
            item("os_updates", "All Windows updates applied — rebooted clean, nothing pending", false),
            item("os_chipset", "Chipset drivers installed", false),
            item("os_gpu", "GPU drivers installed (latest stable)", false),
            item("os_network", "Network / Wi-Fi drivers installed and verified", false),
            item("os_audio", "Audio drivers installed", false),
            item("os_nvme", "NVMe / storage drivers installed (if applicable)", false),
            item("os_utilities", "Xidax software / utilities installed", false),
            item("os_bloatware", "Unnecessary bloatware removed", false),
            item("os_oobe", "Ships at Windows OOBE so customer makes their own account", false),
        ]},
        SectionDef { number: 6, title: "Drive Health", notes: "", items: &[
            item("drv_smart", "SMART status clean on every drive — 0 reallocated / pending sectors", false),
            item("drv_poh", "Power-on hours low (new drive, not a pull)", false),
            item("drv_speed", "Storage read/write within spec for the drive tier — numbers recorded", true),
            item("drv_firmware", "Drive firmware installed/updated where available (WD Black and any drive offering updates)", false),
        ]},
        SectionDef { number: 7, title: "Stress Testing", notes: "", items: &[
            item("st_cpu", "CPU stress test — no errors", false),
            item("st_gpu", "GPU stress test — no artifacts", false),
            item("st_combined", "Combined CPU + GPU load test completed", false),
            item("st_idle_temps", "Idle temps recorded (CPU + GPU)", true),
            item("st_load_temps", "Load temps within safe limits — recorded (CPU + GPU)", true),
            item("st_no_throttle", "No thermal throttling under sustained load", false),
            item("st_no_crash", "No crashes, BSODs, or freezes during testing", false),
            item("st_fan_ramp", "Fan speeds ramp correctly under load", false),
            item("st_noise", "Listen — no coil whine, fan rattle, pump whine, or vibration", false),
        ]},
        SectionDef { number: 8, title: "Benchmarking (record every score to the order)", notes: "", items: &[
            item("bm_gpu", "GPU benchmark — score logged", true),
            item("bm_cpu", "CPU benchmark — score logged", true),
            item("bm_ram", "RAM bandwidth — logged", true),
            item("bm_thresholds", "All scores meet minimum thresholds for the build tier", false),
        ]},
        SectionDef { number: 9, title: "Functional I/O + Network", notes: "", items: &[
            item("io_usb_rear", "Every rear USB port tested with a device", false),
            item("io_usb_front", "Every front USB port tested with a device", false),
            item("io_audio_out", "Audio out tested (rear + front headphone jack)", false),
            item("io_mic", "Mic input tested", false),
            item("io_display", "Each display output tested to a monitor (HDMI / DP)", false),
            item("io_ethernet", "Ethernet — link up and reaches internet", false),
            item("io_wifi", "Wi-Fi — connects and reaches internet", false),
            item("io_bluetooth", "Bluetooth — pairs", false),
        ]},
        SectionDef { number: 10, title: "Final Boot — Customer Experience",
            notes: "Note: the safe-mode-after-power-off check is intentionally omitted — Windows Fast Startup is reset during OOBE, so it can't be confirmed at QC.",
            items: &[
            item("fb_full_shutdown", "Full shutdown (not sleep/hibernate), cut power at the PSU", false),
            item("fb_cold_boot", "Cold boot — powers on, posts clean, lands at Windows OOBE", false),
            item("fb_gpu_display", "GPU driving the display", false),
        ]},
        SectionDef { number: 11, title: "Cosmetic", notes: "", items: &[
            item("cos_exterior", "Chassis exterior free of scratches, dents, blemishes", false),
            item("cos_screws", "All panel screws present and flush", false),
            item("cos_rgb", "RGB lighting working correctly (if applicable)", false),
            item("cos_io_ports", "All I/O ports accessible and undamaged", false),
            item("cos_buttons", "Front panel buttons functional (power, reset)", false),
            item("cos_glass", "Glass / side panel clean inside and out, no fingerprints", false),
        ]},
        SectionDef { number: 12, title: "Packaging & Shipping", notes: "", items: &[
            item("pkg_branding", "Branding applied (BEFORE boxing)", false),
            item("pkg_photos", "Photos added to order: internals, BIOS screen showing RAM speed, benchmark scores, final boot-to-desktop (BEFORE boxing)", false),
            item("pkg_foam", "Foam kit installed — GPU and all components protected and positioned correctly", false),
            item("pkg_bag", "System in anti-static bag", false),
            item("pkg_accessories", "All accessories included — cables, manuals, spare parts, GPU box, extras", false),
            item("pkg_docs", "Customer docs in box: spec sheet, first-boot/setup guide, warranty info", false),
            item("pkg_reseat_note", "\"Re-seat check before first boot\" note included", false),
        ]},
        SectionDef { number: 13, title: "Final Re-Check — Physical Build (re-affirm §1–3)", notes: "", items: &[
            item("rc_hardware", "Re-confirmed component seating, power connectors, and no damage after any rework (§1)", false),
            item("rc_cables", "Cable management still clean — nothing disturbed by later work (§2)", false),
            item("rc_liquid", "Liquid cooling still solid — no leaks or movement after handling (§3; N/A if air-cooled)", false),
        ]},
    ],
};

/// Draft Repair checklist for service orders (lean, repair-focused).
pub static REPAIR: ChecklistDef = ChecklistDef {
    kind: ChecklistKind::Repair,
    sections: &[
        SectionDef { number: 1, title: "Intake & Diagnosis", notes: "", items: &[
            item("rp_intake_notes", "Customer intake notes / reported issue reviewed", false),
            item("rp_device_pw", "Device password on file / confirmed (or noted absent)", false),
            item("rp_physical_damage", "Pre-existing physical damage documented (photos to order)", false),
            item("rp_diagnosis", "Fault reproduced and root cause identified — recorded", true),
        ]},
        SectionDef { number: 2, title: "Repair Performed", notes: "", items: &[
            item("rp_work_done", "Repair work performed — described in notes", true),
            item("rp_parts_replaced", "Parts replaced recorded with serials (old + new)", true),
            item("rp_customer_data", "Customer data handled per policy (preserved / backed up / wiped as authorized)", false),
        ]},
        SectionDef { number: 3, title: "Verification", notes: "", items: &[
            item("rp_smart", "SMART status clean on every drive", false),
            item("rp_stress", "Stress / functional test confirms the fault is resolved", false),
            item("rp_temps", "Temps within safe limits — recorded", true),
            item("rp_no_new_issues", "No new issues introduced — full functional pass", false),
        ]},
        SectionDef { number: 4, title: "Sign-Off / Accountability", notes: "", items: &[
            item("rp_no_blanks", "Every item marked Pass / Fail / N/A — no blanks", false),
            item("rp_fails_noted", "Any Fail has a note: what it was, what was done, re-tested", false),
            item("rp_signed", "Repair tech name + date/time locked on the order in QC Wizard", false),
        ]},
    ],
};

// ─── runtime state (serde + SurrealValue) ────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
#[serde(rename_all = "camelCase")]
pub struct ItemState {
    pub key: String,
    pub text: String,
    pub captures_value: bool,
    /// "Unset" | "Pass" | "Fail" | "NA".
    pub status: String,
    pub note: String,
    pub value: String,
    /// "None" | "Auto" | "Manual".
    pub source: String,
    pub evidence: String,
    pub checked_at: Option<String>,
}

impl ItemState {
    pub fn status(&self) -> ItemStatus {
        ItemStatus::parse(&self.status)
    }

    pub fn auto_verified(&self) -> bool {
        self.source == "Auto"
    }

    pub fn show_note(&self) -> bool {
        self.status == "Fail"
    }

    /// A Fail with no note is not a valid completion.
    pub fn is_resolved(&self) -> bool {
        match self.status() {
            ItemStatus::Unset => false,
            ItemStatus::Fail => !self.note.trim().is_empty(),
            _ => true,
        }
    }

    /// Tech-driven status change: stamps Manual provenance.
    pub fn set_manual(&mut self, status: ItemStatus) {
        self.status = status.as_str().to_string();
        if status == ItemStatus::Unset {
            self.source = CheckSource::None.as_str().to_string();
            self.checked_at = None;
            self.evidence.clear();
        } else {
            self.source = CheckSource::Manual.as_str().to_string();
            self.checked_at = Some(now_stamp());
            self.evidence.clear();
        }
    }

    /// Auto-verified by the app with evidence (SMART/OA3/temps).
    pub fn apply_auto(&mut self, status: ItemStatus, evidence: &str, value: &str) {
        self.source = CheckSource::Auto.as_str().to_string();
        self.evidence = evidence.to_string();
        self.checked_at = Some(now_stamp());
        if !value.is_empty() {
            self.value = value.to_string();
        }
        self.status = status.as_str().to_string();
        if status == ItemStatus::Fail && !value.is_empty() {
            self.note = value.to_string();
        }
    }

    /// Reset a now-stale auto item so the tech must re-check it at sign-off.
    pub fn reset_for_recheck(&mut self, note: &str) {
        self.source = CheckSource::None.as_str().to_string();
        self.evidence.clear();
        self.checked_at = None;
        self.note = note.to_string();
        self.status = ItemStatus::Unset.as_str().to_string();
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
#[serde(rename_all = "camelCase")]
pub struct SectionState {
    pub number: i64,
    pub title: String,
    pub applicable: bool,
    pub notes: String,
    pub completed_at: Option<String>,
    pub items: Vec<ItemState>,
}

impl SectionState {
    pub fn resolved_count(&self) -> usize {
        self.items.iter().filter(|i| i.is_resolved()).count()
    }

    pub fn is_complete(&self) -> bool {
        !self.applicable || self.items.iter().all(|i| i.is_resolved())
    }

    pub fn progress_text(&self) -> String {
        if self.applicable {
            format!("{} / {}", self.resolved_count(), self.items.len())
        } else {
            "N/A".to_string()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistState {
    /// "BuildQC" | "Repair".
    pub kind: String,
    pub sections: Vec<SectionState>,
}

impl ChecklistState {
    pub fn from_def(def: &ChecklistDef) -> Self {
        let sections = def
            .sections
            .iter()
            .map(|s| SectionState {
                number: s.number,
                title: s.title.to_string(),
                applicable: true,
                notes: s.notes.to_string(),
                completed_at: None,
                items: s
                    .items
                    .iter()
                    .map(|i| ItemState {
                        key: i.key.to_string(),
                        text: i.text.to_string(),
                        captures_value: i.captures_value,
                        status: ItemStatus::Unset.as_str().to_string(),
                        source: CheckSource::None.as_str().to_string(),
                        ..Default::default()
                    })
                    .collect(),
            })
            .collect();
        Self { kind: def.kind.as_str().to_string(), sections }
    }

    pub fn from_kind(kind: ChecklistKind) -> Self {
        Self::from_def(def_for(kind))
    }

    pub fn item(&self, key: &str) -> Option<&ItemState> {
        self.sections.iter().flat_map(|s| &s.items).find(|i| i.key == key)
    }

    fn item_mut(&mut self, key: &str) -> Option<&mut ItemState> {
        self.sections
            .iter_mut()
            .flat_map(|s| s.items.iter_mut())
            .find(|i| i.key == key)
    }

    pub fn set_status(&mut self, key: &str, status: ItemStatus) {
        if let Some(i) = self.item_mut(key) {
            i.set_manual(status);
        }
    }

    pub fn set_note(&mut self, key: &str, note: &str) {
        if let Some(i) = self.item_mut(key) {
            i.note = note.to_string();
        }
    }

    pub fn set_value(&mut self, key: &str, value: &str) {
        if let Some(i) = self.item_mut(key) {
            i.value = value.to_string();
        }
    }

    pub fn apply_auto(&mut self, key: &str, status: ItemStatus, evidence: &str, value: &str) {
        if let Some(i) = self.item_mut(key) {
            i.apply_auto(status, evidence, value);
        }
    }

    pub fn reset_for_recheck(&mut self, key: &str, note: &str) {
        if let Some(i) = self.item_mut(key) {
            i.reset_for_recheck(note);
        }
    }

    /// Air-cooled order: mark the Liquid Cooling section N/A (BuildQC §3).
    pub fn set_air_cooled(&mut self, air_cooled: bool) {
        for s in &mut self.sections {
            if s.title.starts_with("Liquid Cooling") {
                s.applicable = !air_cooled;
            }
        }
    }

    /// Overlay saved state onto a fresh checklist, matched by (section number,
    /// item key) so it survives wording/structure tweaks.
    pub fn restore_from(&mut self, saved: &ChecklistState) {
        for sec in &mut self.sections {
            let Some(saved_sec) = saved.sections.iter().find(|s| s.number == sec.number) else {
                continue;
            };
            sec.applicable = saved_sec.applicable;
            if !saved_sec.notes.is_empty() {
                sec.notes = saved_sec.notes.clone();
            }
            sec.completed_at = saved_sec.completed_at.clone();
            for item in &mut sec.items {
                if let Some(saved_item) = saved_sec.items.iter().find(|i| i.key == item.key) {
                    item.status = saved_item.status.clone();
                    item.note = saved_item.note.clone();
                    if !saved_item.value.is_empty() {
                        item.value = saved_item.value.clone();
                    }
                    item.source = saved_item.source.clone();
                    item.evidence = saved_item.evidence.clone();
                    item.checked_at = saved_item.checked_at.clone();
                }
            }
        }
    }

    /// Sign-off is allowed only when every applicable section is complete.
    pub fn is_complete(&self) -> bool {
        self.sections.iter().filter(|s| s.applicable).all(|s| s.is_complete())
    }

    /// Index of the first applicable, incomplete section (the active one).
    pub fn first_incomplete(&self) -> Option<usize> {
        self.sections.iter().position(|s| s.applicable && !s.is_complete())
    }

    /// One `QcFailure` per Fail item, for the post-ship accountability table.
    pub fn failures(&self, order_id: &str) -> Vec<QcFailure> {
        let mut out = Vec::new();
        for sec in &self.sections {
            for item in &sec.items {
                if item.status() == ItemStatus::Fail {
                    out.push(QcFailure {
                        order_id: order_id.to_string(),
                        section_number: sec.number,
                        section_title: sec.title.clone(),
                        item_key: item.key.clone(),
                        item_text: item.text.clone(),
                        note: item.note.clone(),
                        final_status: item.status.clone(),
                    });
                }
            }
        }
        out
    }

    /// (resolved, total) across applicable sections.
    pub fn open_count(&self) -> (usize, usize) {
        let mut resolved = 0;
        let mut total = 0;
        for s in self.sections.iter().filter(|s| s.applicable) {
            resolved += s.resolved_count();
            total += s.items.len();
        }
        (resolved, total)
    }
}

/// One failed checklist item (mirrors QCWizard `qc_sign_off_failure`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
pub struct QcFailure {
    pub order_id: String,
    pub section_number: i64,
    pub section_title: String,
    pub item_key: String,
    pub item_text: String,
    pub note: String,
    pub final_status: String,
}

fn now_stamp() -> String {
    // UTC: the workspace chrono only enables the `now` feature, not `clock`/`Local`.
    chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_qc_has_expected_shape() {
        let c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        assert_eq!(c.kind, "BuildQC");
        assert_eq!(c.sections.len(), 13);
        // Keys are the durable contract — spot-check the auto-verified ones.
        assert!(c.sections.iter().flat_map(|s| &s.items).any(|i| i.key == "os_activated"));
        assert!(c.sections.iter().flat_map(|s| &s.items).any(|i| i.key == "drv_smart"));
        assert!(c.sections.iter().flat_map(|s| &s.items).any(|i| i.key == "st_load_temps"));
        // Every item starts unresolved.
        assert!(!c.is_complete());
    }

    #[test]
    fn fail_needs_a_note_to_resolve() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.set_status("hw_cpu", ItemStatus::Fail);
        let item = c.sections[0].items.iter().find(|i| i.key == "hw_cpu").unwrap();
        assert!(!item.is_resolved(), "Fail without note must not resolve");
        c.set_note("hw_cpu", "wrong CPU shipped");
        let item = c.sections[0].items.iter().find(|i| i.key == "hw_cpu").unwrap();
        assert!(item.is_resolved());
        assert!(item.show_note());
    }

    #[test]
    fn air_cooled_marks_liquid_cooling_na() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.set_air_cooled(true);
        let lc = c.sections.iter().find(|s| s.title.starts_with("Liquid Cooling")).unwrap();
        assert!(!lc.applicable);
        assert!(lc.is_complete(), "non-applicable section is trivially complete");
        assert_eq!(lc.progress_text(), "N/A");
        // Re-arm when toggled back.
        c.set_air_cooled(false);
        let lc = c.sections.iter().find(|s| s.title.starts_with("Liquid Cooling")).unwrap();
        assert!(lc.applicable);
    }

    #[test]
    fn completion_ignores_non_applicable_sections() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.set_air_cooled(true);
        // Pass / NA everything in the applicable sections.
        for s in 0..c.sections.len() {
            if !c.sections[s].applicable {
                continue;
            }
            let keys: Vec<String> = c.sections[s].items.iter().map(|i| i.key.clone()).collect();
            for k in keys {
                c.set_status(&k, ItemStatus::Pass);
            }
        }
        assert!(c.is_complete());
        assert!(c.first_incomplete().is_none());
    }

    #[test]
    fn apply_auto_and_recheck_reset() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.apply_auto("os_activated", ItemStatus::Pass, "Windows OA3 firmware key present", "");
        let i = c.sections.iter().flat_map(|s| &s.items).find(|i| i.key == "os_activated").unwrap();
        assert_eq!(i.status(), ItemStatus::Pass);
        assert!(i.auto_verified());
        c.reset_for_recheck("os_activated", "drifted");
        let i = c.sections.iter().flat_map(|s| &s.items).find(|i| i.key == "os_activated").unwrap();
        assert_eq!(i.status(), ItemStatus::Unset);
        assert!(!i.auto_verified());
        assert_eq!(i.note, "drifted");
    }

    #[test]
    fn failures_collects_only_fails() {
        let mut c = ChecklistState::from_kind(ChecklistKind::BuildQc);
        c.set_status("hw_gpu", ItemStatus::Fail);
        c.set_note("hw_gpu", "artifacting under load");
        c.set_status("hw_cpu", ItemStatus::Pass);
        let f = c.failures("6960322642146");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].item_key, "hw_gpu");
        assert_eq!(f[0].section_number, 1);
        assert_eq!(f[0].note, "artifacting under load");
    }

    #[test]
    fn restore_overlays_by_key() {
        let mut saved = ChecklistState::from_kind(ChecklistKind::BuildQc);
        saved.set_status("hw_cpu", ItemStatus::Pass);
        saved.set_status("hw_gpu", ItemStatus::Fail);
        saved.set_note("hw_gpu", "note");
        saved.set_air_cooled(true);

        let json = serde_json::to_string(&saved).unwrap();
        let parsed: ChecklistState = serde_json::from_str(&json).unwrap();

        let mut fresh = ChecklistState::from_kind(ChecklistKind::BuildQc);
        fresh.restore_from(&parsed);
        let cpu = fresh.sections[0].items.iter().find(|i| i.key == "hw_cpu").unwrap();
        assert_eq!(cpu.status(), ItemStatus::Pass);
        let lc = fresh.sections.iter().find(|s| s.title.starts_with("Liquid Cooling")).unwrap();
        assert!(!lc.applicable, "air-cooled state survives round-trip + restore");
    }

    #[test]
    fn repair_checklist_distinct() {
        let c = ChecklistState::from_kind(ChecklistKind::Repair);
        assert_eq!(c.kind, "Repair");
        assert_eq!(c.sections.len(), 4);
        assert!(c.sections.iter().flat_map(|s| &s.items).any(|i| i.key == "rp_diagnosis"));
    }
}

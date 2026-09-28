//! Fleet pattern matching for a machine at intake: prior crashes of the same
//! signature elsewhere in the fleet, and resolved cases on the same model.
//!
//! Read-only over existing tables (`crash_signature`/`crash_sighting`/
//! `crash_verdict`, `diagnostic_session`, `service_order`, `computer`); no
//! schema migration. The pure ranking and rendering below are unit-tested; the
//! `db()` calls are thin.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use super::crash_intel::machine_crash_history;
use super::{entity_link::canonical_computer_id, RecordId, RecordIdExt};
use crate::db;

/// Sightings of one machine's crashes scanned for fleet matches.
const CRASH_SCAN_LIMIT: u32 = 25;
/// Resolved cases scanned for same-model matches.
const CASE_SCAN_LIMIT: i64 = 400;
/// Similar cases returned by default.
pub const DEFAULT_MAX_CASES: usize = 5;
/// Service orders older than this are not the current job.
pub const DEFAULT_LOOKBACK_DAYS: u32 = 60;
/// Shortest complaint token kept for overlap scoring.
const MIN_TOKEN_LEN: usize = 4;

/// Resolved/escalated cases with their model, complaint and service number
/// pulled through the `computer_id` and `service_order` record links.
pub const SIMILAR_CASES_SQL: &str = "SELECT hostname, summary, diagnosed_at, \
     service_order.service_number AS service_number, \
     service_order.checkin_notes AS checkin_notes, \
     computer_id.product_vendor AS product_vendor, \
     computer_id.product_name AS product_name, \
     computer_id.device_mfg AS device_mfg, \
     computer_id.device_model AS device_model, \
     computer_id AS computer \
     FROM diagnostic_session \
     WHERE status IN ['resolved', 'escalated'] AND summary != NONE AND diagnosed_at != NONE \
     ORDER BY diagnosed_at DESC LIMIT $limit";

/// Complaint words too generic to signal a shared symptom.
const STOPWORDS: &[&str] = &[
    "computer", "laptop", "desktop", "customer", "please", "issue", "issues", "problem", "problems",
    "says", "said", "wont", "want", "need", "needs", "when", "then", "with", "that", "this", "have",
    "from", "into", "your", "will", "just", "very", "back", "after", "before", "about", "there",
    "which", "would", "could", "should", "check", "checked", "running", "getting", "keeps",
];

/// A crash signature this machine shares with others in the fleet.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CrashHit {
    pub bugcheck_code: String,
    pub module: String,
    /// Other fleet machines with this signature (this machine excluded).
    pub other_machines: usize,
    pub sighting_count: u32,
    pub verdict: Option<String>,
    pub fix: Option<String>,
    pub confidence: Option<String>,
}

/// A resolved case on the same model, surfaced as prior art.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SimilarCase {
    pub service_number: String,
    pub hostname: String,
    pub diagnosed_at_unix: Option<i64>,
    pub summary: String,
    /// This case's machine also had one of this machine's crash signatures.
    pub shared_signature: bool,
    /// Complaint words shared with the current ticket.
    pub complaint_overlap: usize,
}

/// What the fleet already knows about a machine like this one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FleetMatch {
    pub model_key: String,
    pub model_label: String,
    pub crash_hits: Vec<CrashHit>,
    pub similar_cases: Vec<SimilarCase>,
}

impl FleetMatch {
    /// True when anything worth surfacing was found.
    pub fn has_signal(&self) -> bool {
        !self.crash_hits.is_empty() || !self.similar_cases.is_empty()
    }

    /// A short "Seen before" staff note; empty when there is no signal.
    pub fn render(&self) -> String {
        if !self.has_signal() {
            return String::new();
        }
        let mut out = String::from("Seen before");
        if !self.model_label.is_empty() {
            out.push_str(&format!(" ({})", self.model_label));
        }
        out.push(':');
        for h in &self.crash_hits {
            out.push_str(&format!("\n• {} {}", h.bugcheck_code, h.module));
            if h.other_machines > 0 {
                out.push_str(&format!(" on {} other machine(s)", h.other_machines));
            }
            match (&h.verdict, &h.fix) {
                (Some(v), Some(f)) if !f.is_empty() => out.push_str(&format!(" — {v}; fix: {f}")),
                (Some(v), _) => out.push_str(&format!(" — {v}")),
                _ => {}
            }
        }
        for c in &self.similar_cases {
            let shared = if c.shared_signature { " [same crash]" } else { "" };
            out.push_str(&format!("\n• #{} {}{}", c.service_number, c.summary, shared));
        }
        out
    }
}

/// Normalized model identity used to group machines. Empty when unknown.
pub fn model_key(vendor: &str, model: &str) -> String {
    let raw = format!("{} {}", vendor.trim(), model.trim());
    let mut words: Vec<String> = raw
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase())
        .filter(|w| {
            !w.is_empty()
                && !matches!(
                    w.as_str(),
                    "inc" | "ltd" | "co" | "corp" | "corporation" | "computer" | "computers"
                        | "technology" | "technologies" | "international" | "gmbh" | "llc"
                )
        })
        .collect();
    words.dedup();
    words.join(" ")
}

/// Human label for a model; falls back to whichever field is present.
pub fn model_label(vendor: &str, model: &str) -> String {
    let joined = format!("{} {}", vendor.trim(), model.trim());
    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Significant lowercase complaint words for overlap scoring.
pub fn complaint_tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= MIN_TOKEN_LEN && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Count of complaint words two tickets share.
pub fn token_overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> usize {
    a.intersection(b).count()
}

/// Orders a case list by shared crash, then complaint overlap, then recency.
fn rank_cases(mut cases: Vec<SimilarCase>, max: usize) -> Vec<SimilarCase> {
    cases.sort_by(|a, b| {
        b.shared_signature
            .cmp(&a.shared_signature)
            .then(b.complaint_overlap.cmp(&a.complaint_overlap))
            .then(b.diagnosed_at_unix.cmp(&a.diagnosed_at_unix))
    });
    cases.truncate(max);
    cases
}

/// A projected string field, empty when absent or null.
pub fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

/// Model key and label from a row's SMBIOS fields, falling back to intake fields.
pub fn model_from_value(v: &serde_json::Value) -> (String, String) {
    let smbios = model_key(&str_field(v, "product_vendor"), &str_field(v, "product_name"));
    if !smbios.is_empty() {
        return (smbios, model_label(&str_field(v, "product_vendor"), &str_field(v, "product_name")));
    }
    (
        model_key(&str_field(v, "device_mfg"), &str_field(v, "device_model")),
        model_label(&str_field(v, "device_mfg"), &str_field(v, "device_model")),
    )
}

/// The bare computer key from a projected record link ("computer:`key`" or a string).
pub fn computer_key_from_value(v: &serde_json::Value, key: &str) -> String {
    let raw = match v.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => return String::new(),
        Some(other) => other.to_string(),
    };
    raw.trim()
        .trim_matches('"')
        .strip_prefix("computer:")
        .unwrap_or(raw.trim().trim_matches('"'))
        .trim_matches('`')
        .to_string()
}

/// Fleet matches for the machine on `connection_string`.
pub async fn fleet_pattern_check(
    connection_string: &str,
    max_cases: usize,
) -> anyhow::Result<FleetMatch> {
    let cs = connection_string.trim();
    if cs.is_empty() {
        anyhow::bail!("connection_string is required");
    }
    let computer = canonical_computer_id(cs);
    let computer_key = computer.key_string();

    let model_rows: Vec<serde_json::Value> = db()
        .query("SELECT product_vendor, product_name, device_mfg, device_model FROM $comp")
        .bind(("comp", computer.clone()))
        .await?
        .take(0)?;
    let (model_key_val, model_label_val) =
        model_rows.first().map(model_from_value).unwrap_or_default();

    let notes: Option<String> = db()
        .query(
            "SELECT VALUE checkin_notes FROM service_order \
             WHERE computer == $comp AND created_at > time::now() - 60d \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(("comp", computer.clone()))
        .await?
        .take(0)?;
    let complaint = complaint_tokens(notes.as_deref().unwrap_or_default());

    let crash_hits = crash_hits_for(&computer, cs).await?;
    let shared_keys = shared_machine_keys(&computer, cs).await?;
    let similar_cases =
        similar_cases_for(&model_key_val, &computer_key, &complaint, &shared_keys, max_cases)
            .await?;

    Ok(FleetMatch {
        model_key: model_key_val,
        model_label: model_label_val,
        crash_hits,
        similar_cases,
    })
}

/// This machine's crash signatures that others in the fleet also hit or have a verdict for.
async fn crash_hits_for(computer: &RecordId, cs: &str) -> anyhow::Result<Vec<CrashHit>> {
    let history = machine_crash_history(Some(computer), cs, CRASH_SCAN_LIMIT).await?;
    let mut hits: Vec<CrashHit> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for sig in &history.signatures {
        if !seen.insert(sig.id.key_string()) {
            continue;
        }
        let others = sig.machines.len().saturating_sub(1);
        let verdict = history
            .verdicts_for(&sig.id)
            .into_iter()
            .max_by_key(|v| v.created_at.to_utc());
        if others == 0 && verdict.is_none() {
            continue;
        }
        hits.push(CrashHit {
            bugcheck_code: sig.bugcheck_code.clone(),
            module: sig.module.clone(),
            other_machines: others,
            sighting_count: sig.sighting_count,
            verdict: verdict.map(|v| v.verdict.clone()),
            fix: verdict.map(|v| v.fix.clone()).filter(|f| !f.is_empty()),
            confidence: verdict.map(|v| v.confidence.clone()),
        });
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.other_machines));
    Ok(hits)
}

/// Computer keys of every machine that shares one of this machine's crash
/// signatures. `crash_signature.machines` holds connection strings, which equal
/// computer keys (`canonical_computer_id`), so they compare directly.
async fn shared_machine_keys(computer: &RecordId, cs: &str) -> anyhow::Result<HashSet<String>> {
    let history = machine_crash_history(Some(computer), cs, CRASH_SCAN_LIMIT).await?;
    let mut keys: HashSet<String> = HashSet::new();
    for sig in &history.signatures {
        for m in &sig.machines {
            keys.insert(m.trim().to_string());
        }
    }
    keys.remove(computer.key_string().as_str());
    keys.remove(cs.trim());
    Ok(keys)
}

/// Resolved cases on the same model, ranked by shared crash and complaint overlap.
async fn similar_cases_for(
    model_key_val: &str,
    self_computer_key: &str,
    complaint: &BTreeSet<String>,
    shared_keys: &HashSet<String>,
    max_cases: usize,
) -> anyhow::Result<Vec<SimilarCase>> {
    if model_key_val.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<serde_json::Value> =
        db().query(SIMILAR_CASES_SQL).bind(("limit", CASE_SCAN_LIMIT)).await?.take(0)?;

    let mut cases: Vec<SimilarCase> = Vec::new();
    for row in &rows {
        let (key, _) = model_from_value(row);
        if key != model_key_val {
            continue;
        }
        let computer_key = computer_key_from_value(row, "computer");
        if computer_key == self_computer_key {
            continue;
        }
        let summary = str_field(row, "summary");
        let service_number = str_field(row, "service_number");
        if summary.trim().is_empty() || service_number.trim().is_empty() {
            continue;
        }
        let diagnosed_at_unix = row
            .get("diagnosed_at")
            .and_then(|x| x.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp());
        cases.push(SimilarCase {
            service_number,
            hostname: str_field(row, "hostname"),
            diagnosed_at_unix,
            complaint_overlap: token_overlap(complaint, &complaint_tokens(&str_field(row, "checkin_notes"))),
            shared_signature: !computer_key.is_empty() && shared_keys.contains(&computer_key),
            summary,
        });
    }
    Ok(rank_cases(cases, max_cases))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_key_normalizes_and_drops_noise() {
        assert_eq!(model_key("ASUSTeK COMPUTER INC.", "ROG Strix G15"), "asustek rog strix g15");
        assert_eq!(model_key("  Dell Inc. ", "XPS 15 9520"), "dell xps 15 9520");
        assert_eq!(model_key("", ""), "");
    }

    #[test]
    fn same_model_keys_match_across_vendor_casing() {
        assert_eq!(model_key("HP", "Victus 15"), model_key("hp", "victus 15"));
    }

    #[test]
    fn complaint_tokens_drop_short_and_stopwords() {
        let t = complaint_tokens("Computer keeps crashing with blue screen after update");
        assert!(t.contains("crashing"));
        assert!(t.contains("blue"));
        assert!(t.contains("screen"));
        assert!(t.contains("update"));
        assert!(!t.contains("keeps"));
        assert!(!t.contains("with"));
        assert!(!t.contains("computer"));
    }

    #[test]
    fn overlap_counts_shared_words() {
        let a = complaint_tokens("random blue screen crashes gaming");
        let b = complaint_tokens("blue screen while gaming heavy load");
        assert_eq!(token_overlap(&a, &b), 3);
    }

    #[test]
    fn ranking_prefers_shared_crash_then_overlap_then_recency() {
        let mk = |sn: &str, shared: bool, overlap: usize, at: i64| SimilarCase {
            service_number: sn.into(),
            hostname: String::new(),
            diagnosed_at_unix: Some(at),
            summary: "x".into(),
            shared_signature: shared,
            complaint_overlap: overlap,
        };
        let ranked = rank_cases(
            vec![
                mk("older", false, 2, 100),
                mk("shared", true, 0, 50),
                mk("newer", false, 2, 200),
            ],
            3,
        );
        assert_eq!(ranked[0].service_number, "shared");
        assert_eq!(ranked[1].service_number, "newer");
        assert_eq!(ranked[2].service_number, "older");
    }

    #[test]
    fn render_empty_without_signal() {
        assert_eq!(FleetMatch::default().render(), "");
        assert!(!FleetMatch::default().has_signal());
    }

    #[test]
    fn render_lists_crashes_and_cases() {
        let m = FleetMatch {
            model_key: "hp victus 15".into(),
            model_label: "HP Victus 15".into(),
            crash_hits: vec![CrashHit {
                bugcheck_code: "0x133".into(),
                module: "nvlddmkm".into(),
                other_machines: 3,
                sighting_count: 7,
                verdict: Some("GPU driver".into()),
                fix: Some("DDU + 552.44".into()),
                confidence: Some("high".into()),
            }],
            similar_cases: vec![SimilarCase {
                service_number: "2155001".into(),
                hostname: "HOST".into(),
                diagnosed_at_unix: Some(1),
                summary: "reseated RAM".into(),
                shared_signature: true,
                complaint_overlap: 2,
            }],
        };
        let text = m.render();
        assert!(text.contains("HP Victus 15"));
        assert!(text.contains("0x133 nvlddmkm on 3 other machine(s)"));
        assert!(text.contains("fix: DDU + 552.44"));
        assert!(text.contains("#2155001 reseated RAM [same crash]"));
    }
}

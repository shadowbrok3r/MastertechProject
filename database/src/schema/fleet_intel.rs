//! Fleet pattern matching for a machine at intake: shared crash signatures and resolved cases on the same model.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use super::crash_intel::{machine_crash_history, MachineCrashHistory};
use super::outcome::record_id_from_string;
use super::{entity_link::canonical_computer_id, RecordId, RecordIdExt, TASK_TABLE, USER_TABLE};
use crate::db;

/// Sightings of one machine's crashes scanned for fleet matches.
const CRASH_SCAN_LIMIT: u32 = 25;
/// Resolved cases scanned for same-model matches.
const CASE_SCAN_LIMIT: i64 = 400;
/// Similar cases returned by default.
pub const DEFAULT_MAX_CASES: usize = 5;
/// Shortest complaint token kept for overlap scoring.
const MIN_TOKEN_LEN: usize = 4;
/// Other machines a crash needs, without a recorded fix, to count in an alert.
const ALERT_MIN_MACHINES: usize = 2;
/// Complaint words a case must share, without a shared crash, to count in an alert.
const ALERT_MIN_OVERLAP: usize = 2;

/// `notification.notification_type` of a fleet pattern alert.
pub const TYPE_FLEET_PATTERN: &str = "Fleet Pattern";
/// `task_note.username` of the fleet pattern note.
pub const FLEET_NOTE_AUTHOR: &str = "Fleet intel";

/// Resolved/escalated cases with model, complaint and service number read through their record links.
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

/// Check-in notes of `$comp`'s newest service order in the last 60 days.
pub const LATEST_COMPLAINT_SQL: &str = "SELECT checkin_notes, created_at FROM service_order \
     WHERE computer = $comp AND created_at > time::now() - 60d ORDER BY created_at DESC LIMIT 1";

/// The open task on `$computer`'s newest service order in the last 45 days; statement 1 holds the row.
pub const OPEN_TICKET_SQL: &str = "LET $order = (SELECT id, created_at FROM service_order \
     WHERE computer = $computer AND created_at > time::now() - 45d ORDER BY created_at DESC LIMIT 1)[0].id; \
     SELECT id AS task, assignee, task_name, service_number, created_at FROM task \
     WHERE $order != NONE AND service_ticket = $order AND completed = false \
     ORDER BY created_at DESC LIMIT 1";

/// Whether `$comp` is a staff or test machine; NONE reads as false.
pub const IS_INTERNAL_SQL: &str = "SELECT VALUE is_internal == true FROM $comp";

/// Text of the ticket's fleet note.
pub const FLEET_NOTE_SQL: &str =
    "SELECT VALUE note FROM task_note WHERE task_id = $task AND username = $author LIMIT 1";

/// Replaces the ticket's fleet note with a new private one; statement 1 holds the id.
pub const POST_FLEET_NOTE_SQL: &str = "DELETE task_note WHERE task_id = $task AND username = $author; \
     CREATE task_note CONTENT { task_id: $task, note: $note, username: $author, user: $user, \
     service_number: $sn, private: true } RETURN VALUE id";

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

    /// Only the crashes and cases strong enough to alert a tech about.
    pub fn strong(&self) -> FleetMatch {
        FleetMatch {
            model_key: self.model_key.clone(),
            model_label: self.model_label.clone(),
            crash_hits: self
                .crash_hits
                .iter()
                .filter(|h| h.fix.is_some() || h.other_machines >= ALERT_MIN_MACHINES)
                .cloned()
                .collect(),
            similar_cases: self
                .similar_cases
                .iter()
                .filter(|c| c.shared_signature || c.complaint_overlap >= ALERT_MIN_OVERLAP)
                .cloned()
                .collect(),
        }
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

    /// One notification line naming what was found and the ticket.
    pub fn headline(&self, ticket: &str) -> String {
        let known = self.crash_hits.iter().filter(|h| h.fix.is_some()).count();
        let repeat = self.crash_hits.len() - known;
        let mut parts: Vec<String> = Vec::new();
        if known > 0 {
            parts.push(plural(known, "crash with a known fix", "crashes with a known fix"));
        }
        if repeat > 0 {
            parts.push(plural(repeat, "crash seen on other machines", "crashes seen on other machines"));
        }
        if !self.similar_cases.is_empty() {
            parts.push(plural(self.similar_cases.len(), "similar resolved case", "similar resolved cases"));
        }
        let model = if self.model_label.is_empty() { String::new() } else { format!(" on {}", self.model_label) };
        format!("Seen before{model}: {} — {ticket}", parts.join(", "))
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The note to post for `strong`; `None` when it is empty or already posted.
pub fn note_to_post(strong: &FleetMatch, existing: Option<&str>) -> Option<String> {
    let note = strong.render();
    if note.is_empty() || existing.map(str::trim) == Some(note.trim()) {
        return None;
    }
    Some(note)
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

fn is_self(machine: &str, self_keys: &[&str]) -> bool {
    self_keys.iter().any(|k| machine.trim() == k.trim())
}

/// This machine's crash signatures that other machines also hit or that carry a verdict.
pub fn crash_hits_from(history: &MachineCrashHistory, self_keys: &[&str]) -> Vec<CrashHit> {
    let mut hits: Vec<CrashHit> = history
        .signatures
        .iter()
        .filter_map(|sig| {
            let others = sig
                .machines
                .iter()
                .map(|m| m.trim())
                .filter(|m| !m.is_empty() && !is_self(m, self_keys))
                .collect::<HashSet<_>>()
                .len();
            let verdict = history.verdicts_for(&sig.id).into_iter().max_by_key(|v| v.created_at.to_utc());
            if others == 0 && verdict.is_none() {
                return None;
            }
            Some(CrashHit {
                bugcheck_code: sig.bugcheck_code.clone(),
                module: sig.module.clone(),
                other_machines: others,
                sighting_count: sig.sighting_count,
                verdict: verdict.map(|v| v.verdict.clone()),
                fix: verdict.map(|v| v.fix.clone()).filter(|f| !f.is_empty()),
                confidence: verdict.map(|v| v.confidence.clone()),
            })
        })
        .collect();
    hits.sort_by_key(|h| std::cmp::Reverse(h.other_machines));
    hits
}

/// Keys of other machines sharing one of this machine's crash signatures (connection strings are computer keys).
pub fn shared_keys_from(history: &MachineCrashHistory, self_keys: &[&str]) -> HashSet<String> {
    history
        .signatures
        .iter()
        .flat_map(|s| s.machines.iter())
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty() && !is_self(m, self_keys))
        .collect()
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

/// A machine's open ticket task and who it is assigned to.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTicket {
    pub task: RecordId,
    pub assignee: RecordId,
    pub task_name: String,
    pub service_number: String,
}

/// An `OPEN_TICKET_SQL` row; `None` without a task id and assignee.
pub fn open_ticket_from_value(v: &serde_json::Value) -> Option<OpenTicket> {
    Some(OpenTicket {
        task: record_id_from_string(TASK_TABLE, &str_field(v, "task"))?,
        assignee: record_id_from_string(USER_TABLE, &str_field(v, "assignee"))?,
        task_name: str_field(v, "task_name"),
        service_number: str_field(v, "service_number"),
    })
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
    let self_keys = [computer_key.as_str(), cs];

    let mut res = db()
        .query("SELECT product_vendor, product_name, device_mfg, device_model FROM $comp")
        .query(LATEST_COMPLAINT_SQL)
        .bind(("comp", computer.clone()))
        .await?;
    let model_rows: Vec<serde_json::Value> = res.take(0)?;
    let complaint_rows: Vec<serde_json::Value> = res.take(1)?;
    let (model_key_val, model_label_val) = model_rows.first().map(model_from_value).unwrap_or_default();
    let complaint =
        complaint_tokens(&complaint_rows.first().map(|r| str_field(r, "checkin_notes")).unwrap_or_default());

    let history = machine_crash_history(Some(&computer), cs, CRASH_SCAN_LIMIT).await?;
    let crash_hits = crash_hits_from(&history, &self_keys);
    let shared_keys = shared_keys_from(&history, &self_keys);
    let similar_cases =
        similar_cases_for(&model_key_val, &computer_key, &complaint, &shared_keys, max_cases).await?;

    Ok(FleetMatch {
        model_key: model_key_val,
        model_label: model_label_val,
        crash_hits,
        similar_cases,
    })
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

/// True for staff and test machines.
pub async fn is_internal(computer: &RecordId) -> anyhow::Result<bool> {
    let flags: Vec<bool> = db().query(IS_INTERNAL_SQL).bind(("comp", computer.clone())).await?.take(0)?;
    Ok(flags.first().copied().unwrap_or(false))
}

/// The open task on the machine's current service order.
pub async fn open_ticket(computer: &RecordId) -> anyhow::Result<Option<OpenTicket>> {
    let rows: Vec<serde_json::Value> =
        db().query(OPEN_TICKET_SQL).bind(("computer", computer.clone())).await?.take(1)?;
    Ok(rows.first().and_then(open_ticket_from_value))
}

/// The ticket's current fleet note, if any.
pub async fn current_fleet_note(task: &RecordId) -> anyhow::Result<Option<String>> {
    let notes: Vec<String> = db()
        .query(FLEET_NOTE_SQL)
        .bind(("task", task.clone()))
        .bind(("author", FLEET_NOTE_AUTHOR))
        .await?
        .take(0)?;
    Ok(notes.into_iter().next())
}

/// Replaces the ticket's fleet note and returns the new note's id.
pub async fn post_fleet_note(
    task: &RecordId,
    service_number: &str,
    user: &RecordId,
    note: &str,
) -> anyhow::Result<RecordId> {
    let mut res = db()
        .query(POST_FLEET_NOTE_SQL)
        .bind(("task", task.clone()))
        .bind(("author", FLEET_NOTE_AUTHOR))
        .bind(("note", note.to_string()))
        .bind(("user", user.clone()))
        .bind(("sn", service_number.to_string()))
        .await?
        .check()?;
    let ids: Vec<RecordId> = res.take(1)?;
    ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("fleet note create returned no id"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CrashSignature, CrashVerdict, Datetime};

    fn hit(machines: usize, fix: Option<&str>) -> CrashHit {
        CrashHit {
            bugcheck_code: "0x133".into(),
            module: "nvlddmkm".into(),
            other_machines: machines,
            sighting_count: 1,
            verdict: fix.map(|_| "GPU driver".to_string()),
            fix: fix.map(str::to_string),
            confidence: None,
        }
    }

    fn case(sn: &str, shared: bool, overlap: usize) -> SimilarCase {
        SimilarCase {
            service_number: sn.into(),
            hostname: String::new(),
            diagnosed_at_unix: Some(1),
            summary: "reseated RAM".into(),
            shared_signature: shared,
            complaint_overlap: overlap,
        }
    }

    fn signature(key: &str, machines: &[&str]) -> CrashSignature {
        let now: Datetime = chrono::Utc::now().into();
        CrashSignature {
            id: RecordId::new("crash_signature", key),
            bugcheck_code: "0x133".into(),
            bugcheck_name: String::new(),
            module: "nvlddmkm".into(),
            offsets: vec![],
            module_versions: vec![],
            failure_buckets: vec![],
            machines: machines.iter().map(|m| m.to_string()).collect(),
            sighting_count: machines.len() as u32,
            first_seen: now.clone(),
            last_seen: now,
            latest_verdict: None,
            tags: vec![],
        }
    }

    fn verdict(signature_key: &str, fix: &str) -> CrashVerdict {
        CrashVerdict {
            id: RecordId::new("crash_verdict", "v1"),
            signature: RecordId::new("crash_signature", signature_key),
            verdict: "GPU driver".into(),
            fix: fix.into(),
            confidence: "high".into(),
            author: String::new(),
            source: "tech".into(),
            task_ref: None,
            created_at: chrono::Utc::now().into(),
        }
    }

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
            diagnosed_at_unix: Some(at),
            ..case(sn, shared, overlap)
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
            crash_hits: vec![CrashHit { other_machines: 3, ..hit(3, Some("DDU + 552.44")) }],
            similar_cases: vec![case("2155001", true, 2)],
        };
        let text = m.render();
        assert!(text.contains("HP Victus 15"));
        assert!(text.contains("0x133 nvlddmkm on 3 other machine(s)"));
        assert!(text.contains("fix: DDU + 552.44"));
        assert!(text.contains("#2155001 reseated RAM [same crash]"));
    }

    #[test]
    fn strong_keeps_known_fixes_and_repeat_crashes_only() {
        let m = FleetMatch {
            crash_hits: vec![hit(0, Some("update BIOS")), hit(1, None), hit(2, None)],
            ..Default::default()
        };
        let s = m.strong();
        assert_eq!(s.crash_hits.len(), 2);
        assert!(s.crash_hits.iter().all(|h| h.fix.is_some() || h.other_machines >= 2));
    }

    #[test]
    fn strong_keeps_shared_or_overlapping_cases_only() {
        let m = FleetMatch {
            similar_cases: vec![case("a", true, 0), case("b", false, 2), case("c", false, 1)],
            ..Default::default()
        };
        let kept: Vec<String> = m.strong().similar_cases.into_iter().map(|c| c.service_number).collect();
        assert_eq!(kept, vec!["a", "b"]);
    }

    #[test]
    fn headline_counts_and_names_the_ticket() {
        let m = FleetMatch {
            model_label: "HP Victus 15".into(),
            crash_hits: vec![hit(0, Some("update BIOS")), hit(3, None)],
            similar_cases: vec![case("a", true, 0), case("b", false, 2)],
            ..Default::default()
        };
        assert_eq!(
            m.headline("Jane Doe - 2155144"),
            "Seen before on HP Victus 15: 1 crash with a known fix, 1 crash seen on other machines, \
             2 similar resolved cases — Jane Doe - 2155144"
        );
    }

    #[test]
    fn note_to_post_skips_empty_and_unchanged() {
        assert_eq!(note_to_post(&FleetMatch::default(), None), None);
        let m = FleetMatch { similar_cases: vec![case("a", true, 0)], ..Default::default() };
        let note = m.render();
        assert_eq!(note_to_post(&m, None), Some(note.clone()));
        assert_eq!(note_to_post(&m, Some(&format!("{note}\n"))), None);
        assert_eq!(note_to_post(&m, Some("Seen before:\n• #old")), Some(note));
    }

    #[test]
    fn crash_hits_exclude_this_machine_and_count_each_other_machine_once() {
        let history = MachineCrashHistory {
            sightings: vec![],
            signatures: vec![
                signature("0x133_nvlddmkm", &["SELF:1", "A:1", "A:1", "B:2"]),
                signature("0x9f_acpi", &["SELF:1"]),
                signature("0xd1_rtwlane", &["SELF:1"]),
            ],
            verdicts: vec![verdict("0xd1_rtwlane", "roll back Realtek Wi-Fi")],
        };
        let hits = crash_hits_from(&history, &["SELF:1"]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].other_machines, 2);
        assert_eq!(hits[0].fix, None);
        assert_eq!(hits[1].other_machines, 0);
        assert_eq!(hits[1].fix.as_deref(), Some("roll back Realtek Wi-Fi"));
    }

    #[test]
    fn shared_keys_exclude_this_machine() {
        let history = MachineCrashHistory {
            signatures: vec![signature("0x133_nvlddmkm", &["SELF:1", "A:1", " B:2 "])],
            ..Default::default()
        };
        let keys = shared_keys_from(&history, &["SELF:1"]);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("A:1") && keys.contains("B:2"));
    }

    #[test]
    fn open_ticket_parses_record_ids() {
        let row = serde_json::json!({
            "task": "task:abc123",
            "assignee": "user:sam",
            "task_name": "Jane Doe - 2155144",
            "service_number": "2155144"
        });
        let t = open_ticket_from_value(&row).expect("ticket");
        assert_eq!(t.task, RecordId::new("task", "abc123"));
        assert_eq!(t.assignee, RecordId::new("user", "sam"));
        assert_eq!(t.task_name, "Jane Doe - 2155144");
        assert!(open_ticket_from_value(&serde_json::json!({ "task": "task:x" })).is_none());
    }
}

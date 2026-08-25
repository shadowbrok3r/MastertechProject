//! Stale diagnostic-session sweep.
//!
//! Every hour, move `open` sessions with no recent activity to `abandoned`.
//! Nothing else closes a session whose agent turn died mid-run: the dispatcher
//! only writes the `assist_request` row, and an operator who walks away leaves
//! one open forever. 38 of 101 sessions were stranded when this was written,
//! the oldest ten days, which the outcome engine cannot see because it only
//! scores `resolved` and `escalated`.
//!
//! `abandoned` rather than a guessed verdict: closing one `resolved` would
//! invent a fix that nobody confirmed, and the outcome engine already ignores
//! any status outside those two, so no rollup change is needed.
//!
//! Keyed on `last_activity_at`, NOT `started_at`. The original version compared
//! `started_at` and so abandoned a session the moment its START passed the
//! threshold, however hard someone was working it. Session 2ca82c42
//! (DESKTOP-9A0JLSE, SO 2138336) was opened 08-22, worked continuously with 30
//! entries, and swept on 08-24 with a stress test mid-flight. Multi-day
//! hardware investigations are normal — waiting on parts, on a bench slot, on
//! an overnight soak. `last_activity_at` is a single indexed field written by
//! every path that records real work, so this answers the correlated-subquery
//! cost (4.7s for three rows) that pushed the original to `started_at`.
//!
//! Three further guards, because a sweep that files real work as "abandoned,
//! no findings" is worse than leaving the row open:
//!  - a session holding a non-closed `ai_task` is never touched; an open
//!    checklist means work is outstanding by definition.
//!  - a session with `diagnosed_at` set but no summary is never touched, and is
//!    logged for a human instead. Auto-filing a diagnosed session as abandoned
//!    produces the worst record in the system, and synthesising a verdict for
//!    it would invent one.
//!  - `ended_at` is derived from the last real activity, never from sweep time,
//!    and is only filled when unset. `swept_at` marks the row so a sweep-written
//!    close is distinguishable from an operator's.
//!
//! Swept rows get a synthesised summary from their own entry roll-up, so the
//! session never reads as "abandoned, no findings" when it holds evidence. The
//! entries themselves are the record; the summary points at them.

use anyhow::Result;
use database::schema::{Datetime, RecordId, RecordIdExt, SurrealValue};
use tokio_cron_scheduler::{Job, JobScheduler};

/// How long a session may sit with no recorded activity before it is abandoned.
const STALE_AFTER: &str = "2d";

/// A stale `open` session and the roll-up its summary is built from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
struct Candidate {
    id: RecordId,
    hostname: String,
    started_at: Datetime,
    #[serde(default)]
    #[surreal(default)]
    last_activity_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    diagnosed_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    summary: Option<String>,
}

/// Per-session entry roll-up: `SELECT ... GROUP BY session_ref` over the
/// candidate set. Not a correlated subquery — one grouped pass, bounded to the
/// ids already selected.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct EntryRollup {
    #[serde(default)]
    entries: i64,
    #[serde(default)]
    first: Option<Datetime>,
    #[serde(default)]
    last: Option<Datetime>,
    #[serde(default)]
    categories: Vec<(String, i64)>,
}

/// Why the sweep left an otherwise-stale session alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keep {
    /// A non-closed `ai_task` hangs off the session.
    OpenTask,
    /// Diagnosed, but no summary — needs a human, not a generic status.
    DiagnosedWithoutSummary,
}

impl Keep {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenTask => "open ai_task",
            Self::DiagnosedWithoutSummary => "diagnosed with no summary",
        }
    }
}

/// True when `s` holds no usable text.
fn is_blank(s: Option<&String>) -> bool {
    s.map(|v| v.trim().is_empty()).unwrap_or(true)
}

/// Whether a stale candidate must be left alone. `has_open_task` comes from the
/// `ai_task` scan; everything else is on the row.
fn keep_reason(c: &Candidate, has_open_task: bool) -> Option<Keep> {
    if has_open_task {
        return Some(Keep::OpenTask);
    }
    if c.diagnosed_at.is_some() && is_blank(c.summary.as_ref()) {
        return Some(Keep::DiagnosedWithoutSummary);
    }
    None
}

/// The activity the sweep judged the session on: its stamp, or `started_at` on
/// rows written before the backfill.
fn effective_activity(c: &Candidate) -> Datetime {
    c.last_activity_at.unwrap_or(c.started_at)
}

/// A summary for a session that really was dropped, built from its own entries
/// so the row is never "abandoned, no findings" while holding evidence.
/// Findings live in the entries; this points at them and says how many.
fn synthesize_summary(c: &Candidate, roll: &EntryRollup) -> String {
    let mut out = format!(
        "ABANDONED BY STALENESS SWEEP: no recorded activity on this session for over {STALE_AFTER}, \
         so it was closed automatically. No verdict was reached and none is implied."
    );
    if roll.entries == 0 {
        out.push_str("\n\nThe session holds no diagnostic entries — nothing was recorded against it.");
        return out;
    }

    let mut cats: Vec<&(String, i64)> = roll.categories.iter().collect();
    cats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let breakdown = cats
        .iter()
        .map(|(name, n)| format!("{n} {name}"))
        .collect::<Vec<_>>()
        .join(", ");

    out.push_str(&format!(
        "\n\nFINDINGS ARE NOT LOST. This session holds {} diagnostic_entry row(s)",
        roll.entries
    ));
    if !breakdown.is_empty() {
        out.push_str(&format!(" ({breakdown})"));
    }
    out.push_str(
        " that survive the sweep and carry whatever was actually established. \
         Read them with get_diagnostic_session rather than concluding nothing was found.",
    );

    if let (Some(first), Some(last)) = (roll.first, roll.last) {
        out.push_str(&format!(
            "\n\nActivity window: {} to {}. Session opened {}.",
            first, last, c.started_at
        ));
    }
    out.push_str(&format!(
        "\n\nHost {}. Status and ended_at on this row were written by the sweep, not by an \
         operator — see swept_at.",
        c.hostname
    ));
    out
}

/// Select stale `open` sessions. Single indexed comparison on
/// `(status, last_activity_at)`; `?? started_at` covers pre-backfill rows.
async fn stale_candidates() -> Result<Vec<Candidate>> {
    let sql = format!(
        "SELECT id, hostname, started_at, last_activity_at, diagnosed_at, summary \
         FROM diagnostic_session \
         WHERE status = 'open' \
           AND (last_activity_at ?? started_at) < time::now() - {STALE_AFTER}"
    );
    let mut res = database::db().query(sql).await?;
    Ok(res.take(0)?)
}

/// Sessions among `ids` that hold a non-closed `ai_task`. One non-correlated
/// query over a small indexed table.
async fn sessions_with_open_task(ids: &[RecordId]) -> Result<Vec<RecordId>> {
    let mut res = database::db()
        .query(
            "SELECT VALUE session_ref FROM ai_task \
             WHERE session_ref IN $ids AND status != 'closed'",
        )
        .bind(("ids", ids.to_vec()))
        .await?;
    Ok(res.take(0)?)
}

/// Entry roll-ups for `ids`, keyed by session. Two grouped passes, both bounded
/// to the candidate set.
async fn entry_rollups(ids: &[RecordId]) -> Result<std::collections::HashMap<RecordId, EntryRollup>> {
    #[derive(serde::Serialize, serde::Deserialize, SurrealValue)]
    struct TotalRow {
        session_ref: RecordId,
        entries: i64,
        first: Option<Datetime>,
        last: Option<Datetime>,
    }
    #[derive(serde::Serialize, serde::Deserialize, SurrealValue)]
    struct CatRow {
        session_ref: RecordId,
        category: String,
        entries: i64,
    }

    let dbh = database::db();
    // `time::max`, not `math::max` — the latter returns null on datetimes.
    let mut totals = dbh
        .query(
            "SELECT session_ref, count() AS entries, time::min(timestamp) AS first, \
             time::max(timestamp) AS last FROM diagnostic_entry \
             WHERE session_ref IN $ids GROUP BY session_ref",
        )
        .bind(("ids", ids.to_vec()))
        .await?;
    let mut out: std::collections::HashMap<RecordId, EntryRollup> = totals
        .take::<Vec<TotalRow>>(0)?
        .into_iter()
        .map(|r| {
            (
                r.session_ref,
                EntryRollup {
                    entries: r.entries,
                    first: r.first,
                    last: r.last,
                    categories: Vec::new(),
                },
            )
        })
        .collect();

    let mut cats = dbh
        .query(
            "SELECT session_ref, category, count() AS entries FROM diagnostic_entry \
             WHERE session_ref IN $ids GROUP BY session_ref, category",
        )
        .bind(("ids", ids.to_vec()))
        .await?;
    for row in cats.take::<Vec<CatRow>>(0)? {
        if let Some(roll) = out.get_mut(&row.session_ref) {
            roll.categories.push((row.category, row.entries));
        }
    }
    Ok(out)
}

/// Abandon one session. `ended_at` is derived from the last real activity and
/// only filled when unset, so a value an operator wrote is never replaced and
/// sweep time never masquerades as the end of the work.
async fn abandon(c: &Candidate, summary: &str, activity: Datetime) -> Result<()> {
    database::db()
        .query(
            "UPDATE $sid SET status = 'abandoned', \
             ended_at = ended_at ?? $activity, \
             summary = $summary, \
             swept_at = time::now() \
             WHERE status = 'open'",
        )
        .bind(("sid", c.id.clone()))
        .bind(("activity", activity))
        .bind(("summary", summary.to_string()))
        .await?;
    Ok(())
}

/// One sweep pass. Returns `(abandoned, kept)` for logging.
async fn sweep_once() -> Result<(usize, Vec<(RecordId, Keep)>)> {
    let candidates = stale_candidates().await?;
    if candidates.is_empty() {
        return Ok((0, Vec::new()));
    }

    let ids: Vec<RecordId> = candidates.iter().map(|c| c.id.clone()).collect();
    let open_task: std::collections::HashSet<RecordId> =
        sessions_with_open_task(&ids).await?.into_iter().collect();

    let mut kept = Vec::new();
    let mut sweepable = Vec::new();
    for c in candidates {
        match keep_reason(&c, open_task.contains(&c.id)) {
            Some(reason) => kept.push((c.id.clone(), reason)),
            None => sweepable.push(c),
        }
    }
    if sweepable.is_empty() {
        return Ok((0, kept));
    }

    let sweep_ids: Vec<RecordId> = sweepable.iter().map(|c| c.id.clone()).collect();
    let rollups = entry_rollups(&sweep_ids).await.unwrap_or_else(|e| {
        log::warn!("stale_session_sweep -> entry roll-up failed, summaries will be sparse: {e}");
        Default::default()
    });

    let mut abandoned = 0;
    for c in &sweepable {
        let roll = rollups.get(&c.id).cloned().unwrap_or_default();
        let summary = synthesize_summary(c, &roll);
        match abandon(c, &summary, effective_activity(c)).await {
            Ok(()) => abandoned += 1,
            Err(e) => log::warn!(
                "stale_session_sweep -> abandoning {} failed: {e}",
                c.id.key_string()
            ),
        }
    }
    Ok((abandoned, kept))
}

/// Register the stale-session sweep on the shared scheduler.
pub async fn register(sched: &JobScheduler) -> Result<()> {
    sched
        .add(Job::new_async("0 7 * * * *", |_, _| {
            Box::pin(async move {
                match sweep_once().await {
                    Ok((0, kept)) if kept.is_empty() => {
                        log::debug!("stale_session_sweep -> nothing stale")
                    }
                    Ok((abandoned, kept)) => {
                        if abandoned > 0 {
                            log::info!("stale_session_sweep -> abandoned {abandoned} session(s)");
                        }
                        // Logged individually: a diagnosed session with no
                        // summary is a record defect waiting on a human, and
                        // nothing else surfaces it.
                        for (id, reason) in kept {
                            log::info!(
                                "stale_session_sweep -> left {} open ({})",
                                id.key_string(),
                                reason.as_str()
                            );
                        }
                    }
                    Err(e) => log::warn!("stale_session_sweep -> query failed: {e}"),
                }
            })
        })?)
        .await?;
    log::info!("axum_server -> stale_session_sweep registered (inactive > {STALE_AFTER})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Datetime {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
            .into()
    }

    fn candidate() -> Candidate {
        Candidate {
            id: RecordId::new("diagnostic_session", "test"),
            hostname: "DESKTOP-TEST".to_string(),
            started_at: ts("2026-08-21T00:00:00Z"),
            last_activity_at: None,
            diagnosed_at: None,
            summary: None,
        }
    }

    // The defect this module was rewritten for: a session worked continuously
    // for days was abandoned because its START was old.
    #[test]
    fn recent_activity_outranks_an_old_start() {
        let mut c = candidate();
        c.started_at = ts("2026-08-21T00:00:00Z");
        c.last_activity_at = Some(ts("2026-08-24T19:04:00Z"));
        // The DB predicate is what excludes it; assert the field the predicate
        // reads is the activity stamp and not the start.
        assert_eq!(effective_activity(&c), ts("2026-08-24T19:04:00Z"));
        assert_ne!(effective_activity(&c), c.started_at);
    }

    #[test]
    fn pre_backfill_row_falls_back_to_started_at() {
        let c = candidate();
        assert_eq!(effective_activity(&c), ts("2026-08-21T00:00:00Z"));
    }

    #[test]
    fn open_ai_task_is_never_swept() {
        let c = candidate();
        assert_eq!(keep_reason(&c, true), Some(Keep::OpenTask));
    }

    #[test]
    fn open_task_wins_even_when_otherwise_sweepable() {
        let mut c = candidate();
        c.summary = Some("a real summary".to_string());
        c.diagnosed_at = Some(ts("2026-08-21T22:08:22Z"));
        assert_eq!(keep_reason(&c, true), Some(Keep::OpenTask));
    }

    #[test]
    fn diagnosed_without_summary_is_never_swept() {
        let mut c = candidate();
        c.diagnosed_at = Some(ts("2026-08-21T22:08:22Z"));
        assert_eq!(
            keep_reason(&c, false),
            Some(Keep::DiagnosedWithoutSummary)
        );
    }

    #[test]
    fn whitespace_only_summary_counts_as_no_summary() {
        let mut c = candidate();
        c.diagnosed_at = Some(ts("2026-08-21T22:08:22Z"));
        c.summary = Some("   \n ".to_string());
        assert_eq!(
            keep_reason(&c, false),
            Some(Keep::DiagnosedWithoutSummary)
        );
    }

    #[test]
    fn diagnosed_with_a_summary_is_sweepable() {
        let mut c = candidate();
        c.diagnosed_at = Some(ts("2026-08-21T22:08:22Z"));
        c.summary = Some("root cause established".to_string());
        assert_eq!(keep_reason(&c, false), None);
    }

    #[test]
    fn undiagnosed_and_untasked_is_sweepable() {
        assert_eq!(keep_reason(&candidate(), false), None);
    }

    // ended_at must describe when work stopped, not when the cron ran.
    #[test]
    fn ended_at_is_derived_from_activity_not_sweep_time() {
        let mut c = candidate();
        c.last_activity_at = Some(ts("2026-08-22T21:22:12Z"));
        let derived = effective_activity(&c);
        assert_eq!(derived, ts("2026-08-22T21:22:12Z"));
        // Whatever "now" is when the sweep runs, the written value is the
        // activity stamp — the UPDATE binds this, never time::now().
        assert!(chrono::DateTime::<chrono::Utc>::from(derived) < chrono::Utc::now());
    }

    #[test]
    fn swept_session_keeps_a_usable_record() {
        let c = candidate();
        let roll = EntryRollup {
            entries: 43,
            first: Some(ts("2026-08-21T21:48:51Z")),
            last: Some(ts("2026-08-21T23:55:00Z")),
            categories: vec![
                ("finding".to_string(), 12),
                ("recommendation".to_string(), 28),
                ("action".to_string(), 3),
            ],
        };
        let s = synthesize_summary(&c, &roll);
        assert!(s.contains("43 diagnostic_entry row(s)"));
        assert!(s.contains("28 recommendation"));
        assert!(s.contains("12 finding"));
        assert!(s.contains("get_diagnostic_session"));
        assert!(s.contains("DESKTOP-TEST"));
        assert!(!s.is_empty());
    }

    #[test]
    fn synthesized_summary_claims_no_verdict() {
        let s = synthesize_summary(&candidate(), &EntryRollup { entries: 5, ..Default::default() });
        assert!(s.contains("No verdict was reached and none is implied."));
    }

    #[test]
    fn empty_session_summary_says_so_instead_of_pointing_at_entries() {
        let s = synthesize_summary(&candidate(), &EntryRollup::default());
        assert!(s.contains("holds no diagnostic entries"));
        assert!(!s.contains("FINDINGS ARE NOT LOST"));
    }

    #[test]
    fn category_breakdown_is_ordered_by_count() {
        let roll = EntryRollup {
            entries: 6,
            categories: vec![("note".to_string(), 1), ("finding".to_string(), 5)],
            ..Default::default()
        };
        let s = synthesize_summary(&candidate(), &roll);
        let finding = s.find("5 finding").unwrap();
        let note = s.find("1 note").unwrap();
        assert!(finding < note);
    }
}

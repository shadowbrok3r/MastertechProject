//! Human-readable `sync` and `--revert` reports.

use std::fmt::Write as _;

use crate::apply::{RowSet, SyncRecord, TaskHandling, Write};
use crate::plan::{
    DeactivateReason, Found, LookupMethod, OpenTask, Plan, StoreMove, TaskAction, UserRef, rid,
};

/// Open task ids printed per user before the rest are counted.
const TASK_ID_LIMIT: usize = 10;

/// `NONE`, `''` or the value.
fn shop(id_store: Option<&str>) -> String {
    match id_store {
        None => "NONE".into(),
        Some("") => "''".into(),
        Some(value) => value.into(),
    }
}

fn place(user: &UserRef) -> String {
    format!("{}/{}", user.store, shop(user.id_store.as_deref()))
}

fn tasks_line(out: &mut String, tasks: &[OpenTask]) {
    if tasks.is_empty() {
        return;
    }
    let shown: Vec<String> = tasks
        .iter()
        .take(TASK_ID_LIMIT)
        .map(|task| match &task.service_number {
            Some(sn) => format!("{} (SO {sn})", rid(&task.id)),
            None => rid(&task.id),
        })
        .collect();
    let more = tasks.len().saturating_sub(TASK_ID_LIMIT);
    let suffix = if more > 0 {
        format!(", and {more} more")
    } else {
        String::new()
    };
    let _ = writeln!(out, "      open: {}{suffix}", shown.join(", "));
}

fn method(method: LookupMethod) -> &'static str {
    match method {
        LookupMethod::Id => "by id",
        LookupMethod::Email => "by email",
        LookupMethod::Skipped => "not looked up",
    }
}

fn move_line(out: &mut String, m: &StoreMove) {
    let _ = writeln!(
        out,
        "  {:<34} {} -> {}/{}  {}  ps#{} ({})  open tasks {}",
        m.user.email,
        place(&m.user),
        m.store.as_str(),
        m.id_store,
        m.user.authorization,
        m.employee_id,
        method(m.method),
        m.open_tasks.len()
    );
    tasks_line(out, &m.open_tasks);
}

fn found_lines(
    out: &mut String,
    heading: &str,
    items: &[Found],
    detail: impl Fn(&Found) -> String,
) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(out, "  {heading}");
    for item in items {
        let _ = writeln!(out, "    {:<34} {}", item.user.email, detail(item));
    }
}

/// The dry-run or apply report for `plan`.
pub fn render(plan: &Plan, apply: bool) -> String {
    let mut out = String::new();
    let mode = if apply { "APPLY" } else { "DRY RUN, no writes" };
    let _ = writeln!(
        out,
        "user-admin sync: {mode}. {} users ({} active); {} looked up by id, {} by email.",
        plan.users, plan.active_users, plan.by_id, plan.by_email
    );
    if plan.allow_war {
        let _ = writeln!(out, "--allow-war is set: moves to WAR are included below.");
    }

    if !plan.deactivate.is_empty() {
        let _ = writeln!(out, "\nDEACTIVATE (writes active = false)");
        for d in &plan.deactivate {
            let why = match &d.reason {
                DeactivateReason::PrestashopInactive { employee_id } => {
                    format!("ps#{employee_id} active=0")
                }
                DeactivateReason::NamedMissing => "--deactivate-missing".into(),
            };
            let tasks = match &d.tasks {
                TaskAction::Untouched => String::new(),
                TaskAction::Reassign { to_email, .. } => format!(" -> reassign to {to_email}"),
                TaskAction::Orphan => " (left on the inactive user)".into(),
            };
            let _ = writeln!(
                out,
                "  {:<34} {}  {}  {why}  open tasks {}{tasks}",
                d.user.email,
                place(&d.user),
                d.user.authorization,
                d.open_tasks.len()
            );
            tasks_line(&mut out, &d.open_tasks);
        }
    }
    if !plan.move_store.is_empty() {
        let _ = writeln!(out, "\nMOVE STORE (writes id_store and store)");
        for m in &plan.move_store {
            move_line(&mut out, m);
        }
    }
    if !plan.held_war.is_empty() {
        let _ = writeln!(out, "\nHELD WAR (needs --allow-war; no write)");
        for m in &plan.held_war {
            move_line(&mut out, m);
        }
    }
    if !plan.unmapped.is_empty() {
        let _ = writeln!(out, "\nUNMAPPED PrestaShop store id (no write)");
        for u in &plan.unmapped {
            let _ = writeln!(
                out,
                "  {:<34} {}  ps#{} id_store {}",
                u.user.email,
                place(&u.user),
                u.employee_id,
                u.id_store
            );
        }
    }
    if !plan.not_found.is_empty() {
        let _ = writeln!(
            out,
            "\nNOT FOUND in the directory (deactivate only with --deactivate-missing <email>)"
        );
        for n in &plan.not_found {
            let how = match n.method {
                LookupMethod::Id => "id lookup found nothing",
                LookupMethod::Email => "no id_prestashop; email lookup found nothing",
                LookupMethod::Skipped => "not looked up",
            };
            let _ = writeln!(
                out,
                "  {:<34} {}  {}  {how}",
                n.user.email,
                place(&n.user),
                n.user.authorization
            );
        }
    }
    if !plan.skipped.is_empty() {
        let _ = writeln!(out, "\nSKIPPED (--skip)");
        for u in &plan.skipped {
            let _ = writeln!(out, "  {:<34} {}", u.email, place(u));
        }
    }

    let r = &plan.report;
    let any_report = !(r.inactive_but_employed.is_empty()
        && r.email_drift.is_empty()
        && r.name_drift.is_empty()
        && r.invalid_store.is_empty()
        && r.duplicate_names.is_empty()
        && r.unlinked_matches.is_empty());
    if any_report {
        let _ = writeln!(out, "\nREPORT ONLY (never written)");
        found_lines(
            &mut out,
            "inactive here, active in the directory (never reactivated):",
            &r.inactive_but_employed,
            |f| format!("ps#{} {}", f.employee.id, f.employee.id_store),
        );
        found_lines(&mut out, "email drift:", &r.email_drift, |f| {
            format!("directory has {}", f.employee.email)
        });
        found_lines(&mut out, "name drift:", &r.name_drift, |f| {
            format!("stored {:?}, directory {:?}", f.user.name, f.employee.name)
        });
        if !r.invalid_store.is_empty() {
            let _ = writeln!(out, "  invalid store codes:");
            for u in &r.invalid_store {
                let state = if u.active { "active" } else { "inactive" };
                let _ = writeln!(out, "    {:<34} {:?} ({state})", u.email, u.store);
            }
        }
        if !r.duplicate_names.is_empty() {
            let _ = writeln!(out, "  duplicate names:");
            for dup in &r.duplicate_names {
                let _ = writeln!(out, "    {:?}: {}", dup.name, dup.emails.join(", "));
            }
        }
        found_lines(
            &mut out,
            "no id_prestashop, matched by email (id not written):",
            &r.unlinked_matches,
            |f| format!("ps#{}", f.employee.id),
        );
    }

    if plan.refused.is_empty() {
        let _ = writeln!(out, "\nREFUSED: none");
    } else {
        let _ = writeln!(out, "\nREFUSED (--apply is blocked)");
        for refusal in &plan.refused {
            let _ = writeln!(out, "  {refusal}");
        }
    }

    let counts = format!(
        "{} deactivation(s) and {} store move(s)",
        plan.deactivate.len(),
        plan.move_store.len()
    );
    if plan.is_empty() {
        let _ = writeln!(out, "Nothing to write.");
    } else if !apply {
        let _ = writeln!(out, "Re-run with --apply to write {counts}.");
    } else if plan.refused.is_empty() {
        let _ = writeln!(out, "Writing {counts} in one transaction.");
    }
    out
}

/// The preview or confirmation of a `--revert`.
pub fn render_revert(record: &SyncRecord, apply: bool) -> String {
    let mut out = String::new();
    let mode = if apply { "APPLY" } else { "DRY RUN, no writes" };
    let _ = writeln!(
        out,
        "user-admin sync --revert: {mode}. Sync of {} (committed {}) against {}; {} write(s) to undo.",
        record.created_at,
        record.committed_at.as_deref().unwrap_or("never"),
        record.target,
        record.writes.len()
    );
    for write in record.writes.iter().rev() {
        let inverse = write.inverse();
        let before = format!(
            "{}/{}",
            inverse.expect.store,
            shop(inverse.expect.id_store.as_deref())
        );
        match (&inverse.set, write) {
            (RowSet::Active(_), Write::Deactivate { tasks, .. }) => {
                let _ = writeln!(out, "  reactivate    {:<34} {before}", write.email());
                if let TaskHandling::Reassign {
                    to_email, tasks, ..
                } = tasks
                {
                    let _ = writeln!(
                        out,
                        "    move back up to {} task(s) still open on {to_email}",
                        tasks.len()
                    );
                }
            }
            (RowSet::Store { store, id_store }, _) => {
                let _ = writeln!(
                    out,
                    "  restore store {:<34} {before} -> {store}/{}",
                    write.email(),
                    shop(id_store.as_deref())
                );
            }
            (RowSet::Active(_), Write::MoveStore { .. }) => {}
        }
    }
    if !apply {
        let _ = writeln!(out, "Re-run with --apply to write this revert.");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::apply::{Target, writes};
    use crate::plan::tests::{found, missing, open, staff, user};
    use crate::plan::{SyncOptions, plan};

    fn sample() -> Plan {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let matt = user("matt", "ORE", Some("12"), Some(12));
        let bstair = user("bstair", "RIV", Some("1"), Some(1));
        let carson = user("carson", "MUR", Some("11"), Some(11));
        let johan = user("johanmena", "RIV", None, None);
        let tasks = HashMap::from([open(&jane, &["t1", "t2"])]);
        let mut entries = staff();
        entries.extend([
            found(jane, "10", false),
            found(matt, "12", true),
            found(bstair, "1", true),
            found(carson, "11", true),
            missing(johan),
        ]);
        plan(&entries, &tasks, &SyncOptions::default())
    }

    #[test]
    fn the_dry_run_lists_every_section_and_the_refusals() {
        let text = render(&sample(), false);
        assert!(text.starts_with(
            "user-admin sync: DRY RUN, no writes. 15 users (15 active); 14 looked up by id, 1 by email."
        ));
        for heading in [
            "DEACTIVATE",
            "MOVE STORE",
            "HELD WAR",
            "UNMAPPED",
            "NOT FOUND",
            "REFUSED (--apply is blocked)",
        ] {
            assert!(text.contains(heading), "{heading}\n{text}");
        }
        assert!(text.contains("jane@pclaptops.com"));
        assert!(text.contains("task:t1 (SO SO-t1), task:t2 (SO SO-t2)"));
        assert!(text.contains("ORE/12 -> SAN/12"));
        assert!(text.contains("RIV/1 -> WAR/1"));
        assert!(text.contains("has 2 open task(s)"));
        assert!(
            text.contains("Re-run with --apply to write 1 deactivation(s) and 1 store move(s).")
        );
    }

    #[test]
    fn an_in_sync_plan_reports_nothing_to_write() {
        let text = render(
            &plan(&staff(), &HashMap::new(), &SyncOptions::default()),
            false,
        );
        assert!(text.contains("REFUSED: none"));
        assert!(text.contains("Nothing to write."));
        assert!(!text.contains("DEACTIVATE"));
    }

    #[test]
    fn the_revert_preview_lists_the_inverse_writes() {
        let plan = sample();
        let record = SyncRecord {
            format: 1,
            created_at: "2026-09-26T10:00:00Z".into(),
            committed_at: Some("2026-09-26T10:00:01Z".into()),
            reverted_at: None,
            target: Target {
                url: "127.0.0.1:8000".into(),
                local: true,
                ns: "ns".into(),
                db: "db".into(),
            },
            writes: writes(&plan),
            plan,
        };
        let text = render_revert(&record, false);
        assert!(text.contains("2 write(s) to undo"), "{text}");
        assert!(text.contains("(committed 2026-09-26T10:00:01Z)"), "{text}");
        assert!(text.contains("reactivate    jane@pclaptops.com"));
        assert!(text.contains("restore store matt@pclaptops.com"));
        assert!(text.contains("SAN/12 -> ORE/12"));
        assert!(text.contains("Re-run with --apply"));
    }
}

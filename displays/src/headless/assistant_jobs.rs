//! Fires due task schedules, un-snoozes notifications, escalates overdue assistant tasks and sends the morning brief.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use database::schema::assistant::{
    AssignedTask, OverdueTask, Person, TYPE_MORNING_BRIEF, TYPE_OVERDUE, escalation_contacts_from, notify, unsnooze_due,
};
use database::schema::business_calendar::{CLOSE_HOUR, OPEN_HOUR, STORE_TZ, is_open_day};
use database::schema::morning_brief::{briefed_today, build_all};
use database::schema::task_schedule::{TaskSchedule, due_for_run, local_label, schema_applied};
use database::schema::{RecordId, RecordIdExt};

/// Seconds between passes.
const TICK_SECS: u64 = 60;
/// Schedules fired per pass at most.
const FIRE_BATCH: i64 = 25;
/// Passes between overdue checks.
const ESCALATE_EVERY: u64 = 10;
/// Grace after a due time before anyone is told.
const OVERDUE_GRACE_MINS: i64 = 60;
/// Local hour after which a missed morning brief is no longer sent.
const BRIEF_LAST_HOUR: u32 = 12;
/// Passes between checks for the schedule table while it is missing.
const SCHEMA_RECHECK_EVERY: u64 = 10;

/// Starts the jobs unless `MTECH_ASSISTANT_JOBS` is `0` or `off`.
pub fn spawn_assistant_jobs() {
    let disabled = std::env::var("MTECH_ASSISTANT_JOBS")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false"))
        .unwrap_or(false);
    if disabled {
        log::info!("assistant jobs: disabled by MTECH_ASSISTANT_JOBS");
        return;
    }
    tokio::spawn(async {
        let mut tick = tokio::time::interval(Duration::from_secs(TICK_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut briefed_on: Option<NaiveDate> = None;
        let mut schedules_ready = false;
        let mut pass: u64 = 0;
        loop {
            tick.tick().await;
            let now = Utc::now();
            if !schedules_ready && pass.is_multiple_of(SCHEMA_RECHECK_EVERY) {
                schedules_ready = schema_applied().await.unwrap_or(false);
                if !schedules_ready && pass == 0 {
                    log::info!("assistant jobs: task_schedule is not defined yet; schedules stay off until it is");
                }
            }
            if schedules_ready && let Err(e) = fire_due_schedules(now).await {
                log::warn!("assistant jobs: firing schedules failed: {e}");
            }
            match unsnooze_due().await {
                Ok(0) => {}
                Ok(n) => log::info!("assistant jobs: {n} snoozed notification(s) back to unread"),
                Err(e) => log::warn!("assistant jobs: unsnooze failed: {e}"),
            }
            if pass.is_multiple_of(ESCALATE_EVERY)
                && store_is_open(now)
                && let Err(e) = escalate_overdue(now).await
            {
                log::warn!("assistant jobs: overdue escalation failed: {e}");
            }
            if brief_window(now) && briefed_on != Some(local_date(now)) {
                match send_morning_briefs(now).await {
                    Ok(sent) => {
                        briefed_on = Some(local_date(now));
                        log::info!("assistant jobs: sent {sent} morning brief(s)");
                    }
                    Err(e) => log::warn!("assistant jobs: morning brief failed: {e}"),
                }
            }
            pass = pass.wrapping_add(1);
        }
    });
}

fn local_date(now: DateTime<Utc>) -> NaiveDate {
    now.with_timezone(&STORE_TZ).date_naive()
}

fn store_is_open(now: DateTime<Utc>) -> bool {
    let local = now.with_timezone(&STORE_TZ);
    is_open_day(local.weekday()) && (OPEN_HOUR..CLOSE_HOUR).contains(&local.hour())
}

/// Store-open morning on an open day, until [`BRIEF_LAST_HOUR`].
fn brief_window(now: DateTime<Utc>) -> bool {
    let local = now.with_timezone(&STORE_TZ);
    is_open_day(local.weekday()) && (OPEN_HOUR..BRIEF_LAST_HOUR).contains(&local.hour())
}

/// Creates one task per due schedule; a run another worker claimed is skipped.
async fn fire_due_schedules(now: DateTime<Utc>) -> anyhow::Result<()> {
    for schedule in TaskSchedule::due(FIRE_BATCH).await? {
        let Some(expected) = schedule.next_run else { continue };
        let assignee = Person::load(&schedule.assignee).await?;
        if !assignee.as_ref().is_some_and(|p| p.active) {
            TaskSchedule::cancel(&schedule.id).await?;
            log::info!("assistant jobs: stopped schedule {} for an inactive assignee", schedule.id.key_string());
            continue;
        }
        if !schedule.claim_run(&expected, now).await? {
            continue;
        }
        let task = AssignedTask {
            name: schedule.title.clone(),
            description: schedule.description.clone().unwrap_or_default(),
            assignee: schedule.assignee.clone(),
            assigned_by: schedule.created_by.clone(),
            due: due_for_run(now, schedule.due_hours),
            priority: schedule.priority.clone().unwrap_or_else(|| "Normal".to_string()),
            schedule: Some(schedule.id.clone()),
            about_service: schedule.service_number.clone(),
            part_request: None,
        };
        match task.create().await {
            Ok(id) => log::info!(
                "assistant jobs: schedule {} created task {} for {}",
                schedule.id.key_string(),
                id.key_string(),
                schedule.assignee.key_string()
            ),
            Err(e) => log::warn!("assistant jobs: schedule {} run lost: {e}", schedule.id.key_string()),
        }
    }
    Ok(())
}

/// Who hears about an overdue task: the assignee for their own reminder, else the assigner plus store managers.
#[allow(clippy::mutable_key_type)]
fn overdue_recipients(task: &OverdueTask, people: &[Person]) -> Vec<RecordId> {
    let own = task.assigned_by.as_ref().is_none_or(|by| *by == task.assignee);
    if own {
        return vec![task.assignee.clone()];
    }
    let mut out: Vec<RecordId> = Vec::new();
    let mut seen: HashSet<RecordId> = HashSet::from([task.assignee.clone()]);
    let store = task.store.clone().unwrap_or_default();
    let contacts = escalation_contacts_from(people, &store).into_iter().map(|p| p.id);
    for id in task.assigned_by.iter().cloned().chain(contacts) {
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    out
}

async fn escalate_overdue(now: DateTime<Utc>) -> anyhow::Result<()> {
    let overdue = OverdueTask::unescalated(now - chrono::Duration::minutes(OVERDUE_GRACE_MINS)).await?;
    if overdue.is_empty() {
        return Ok(());
    }
    let people = Person::active_all().await?;
    for task in overdue {
        let due = task.due_date.map(|d| local_label(d.into_inner())).unwrap_or_default();
        let who = task.assignee_name.clone().unwrap_or_else(|| "someone".to_string());
        for user in overdue_recipients(&task, &people) {
            let text = if user == task.assignee {
                format!("Overdue: {} (due {due})", task.task_name)
            } else {
                format!("Overdue: {} ({who}, due {due})", task.task_name)
            };
            notify(&user, TYPE_OVERDUE, &text, task.assigned_by.as_ref(), Some(&task.id)).await?;
        }
        task.mark_escalated().await?;
    }
    Ok(())
}

#[allow(clippy::mutable_key_type)]
async fn send_morning_briefs(now: DateTime<Utc>) -> anyhow::Result<usize> {
    let already = briefed_today(now).await?;
    let mut sent = 0;
    for (person, text) in build_all(now).await? {
        if already.contains(&person.id) {
            continue;
        }
        notify(&person.id, TYPE_MORNING_BRIEF, &text, None, None).await?;
        sent += 1;
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn person(key: &str, store: &str, auth: &str) -> Person {
        Person {
            id: RecordId::new("user", key),
            name: key.into(),
            email: format!("{key}@pclaptops.com"),
            store: store.into(),
            authorization: auth.into(),
            active: true,
        }
    }

    fn overdue(assignee: &str, by: Option<&str>) -> OverdueTask {
        OverdueTask {
            id: RecordId::new("task", "t"),
            task_name: "Count paste".into(),
            assignee: RecordId::new("user", assignee),
            assigned_by: by.map(|b| RecordId::new("user", b)),
            due_date: None,
            assignee_name: Some(assignee.into()),
            store: Some("RIV".into()),
        }
    }

    #[test]
    fn brief_window_is_store_open_morning_on_open_days() {
        assert!(brief_window(utc("2026-09-28T16:05:00Z")));
        assert!(!brief_window(utc("2026-09-28T15:59:00Z")));
        assert!(!brief_window(utc("2026-09-28T18:00:00Z")));
        assert!(!brief_window(utc("2026-09-27T16:30:00Z")));
    }

    #[test]
    fn own_reminders_escalate_to_the_assignee_only() {
        let people = vec![person("ana", "RIV", "Manager"), person("logan", "RIV", "Root")];
        assert_eq!(overdue_recipients(&overdue("sam", Some("sam")), &people), vec![RecordId::new("user", "sam")]);
        assert_eq!(overdue_recipients(&overdue("sam", None), &people), vec![RecordId::new("user", "sam")]);
    }

    #[test]
    fn assigned_work_escalates_to_the_assigner_and_store_managers_once() {
        let people = vec![person("ana", "RIV", "Manager"), person("logan", "RIV", "Root")];
        let to = overdue_recipients(&overdue("sam", Some("logan")), &people);
        assert_eq!(to, vec![RecordId::new("user", "logan"), RecordId::new("user", "ana")]);
        let by_manager = overdue_recipients(&overdue("sam", Some("ana")), &people);
        assert_eq!(by_manager, vec![RecordId::new("user", "ana")]);
    }
}

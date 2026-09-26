//! Recurring and one-off task schedules; the admin-agent turns each due run into a `task`.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};

use super::business_calendar::{CLOSE_HOUR, STORE_TZ, is_open_day};
use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const TASK_SCHEDULE_TABLE: &str = "task_schedule";

/// Longest schedule title, in characters.
pub const TITLE_MAX_CHARS: usize = 120;
/// Longest schedule description, in characters.
pub const DESCRIPTION_MAX_CHARS: usize = 1000;
/// Largest `interval` a schedule accepts.
pub const MAX_INTERVAL: u32 = 12;

/// Active schedules whose next run is due, oldest first.
pub const DUE_SCHEDULES_SQL: &str = "SELECT * FROM task_schedule \
     WHERE active = true AND next_run != NONE AND next_run <= time::now() \
     ORDER BY next_run ASC LIMIT $limit";

/// Moves `$id` past the run at `$expected`; returns the id only when this caller won the run.
pub const CLAIM_RUN_SQL: &str = "UPDATE $id SET next_run = $next, last_run = $fired, runs += 1, \
     active = $active WHERE active = true AND next_run = $expected RETURN VALUE id";

/// Active schedules assigned to or created by `$user`, soonest first.
pub const SCHEDULES_FOR_USER_SQL: &str = "SELECT * FROM task_schedule \
     WHERE active = true AND (assignee = $user OR created_by = $user) ORDER BY next_run ASC";

/// How often a schedule repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Every {
    Once,
    Day,
    Week,
    Month,
}

impl Every {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "once" => Some(Self::Once),
            "day" | "daily" => Some(Self::Day),
            "week" | "weekly" => Some(Self::Week),
            "month" | "monthly" => Some(Self::Month),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
}

/// A validated repeat rule; times are store-local.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recurrence {
    pub every: Every,
    pub interval: u32,
    pub weekdays: Vec<Weekday>,
    pub month_day: Option<u32>,
    pub at: NaiveTime,
}

/// `HH:MM` as a time of day.
pub fn parse_hhmm(raw: &str) -> Option<NaiveTime> {
    let (h, m) = raw.trim().split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

/// ISO weekday number (1 = Monday .. 7 = Sunday) as a chrono weekday.
pub fn weekday_from_iso(n: i64) -> Option<Weekday> {
    match n {
        1 => Some(Weekday::Mon),
        2 => Some(Weekday::Tue),
        3 => Some(Weekday::Wed),
        4 => Some(Weekday::Thu),
        5 => Some(Weekday::Fri),
        6 => Some(Weekday::Sat),
        7 => Some(Weekday::Sun),
        _ => None,
    }
}

/// Weekday from a name or abbreviation, or an ISO number as text.
pub fn weekday_from_name(raw: &str) -> Option<Weekday> {
    let lower = raw.trim().to_ascii_lowercase();
    let s = lower.trim_end_matches(['s', '.']);
    if let Ok(n) = s.parse::<i64>() {
        return weekday_from_iso(n);
    }
    let day = match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    };
    Some(day)
}

/// A store-local wall-clock time as UTC; a DST gap resolves one hour later.
pub fn local_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    STORE_TZ
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| STORE_TZ.from_local_datetime(&(naive + Duration::hours(1))).earliest())
        .map(|t| t.with_timezone(&Utc))
}

/// Monday-based week number since 1970-01-05.
fn week_index(date: NaiveDate) -> i64 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 5).expect("valid epoch");
    (date - epoch).num_days().div_euclid(7)
}

fn month_index(date: NaiveDate) -> i64 {
    date.year() as i64 * 12 + date.month0() as i64
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).and_then(|d| d.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

/// The next open day on or after `date`.
fn roll_to_open_day(mut date: NaiveDate) -> NaiveDate {
    while !is_open_day(date.weekday()) {
        match date.succ_opt() {
            Some(next) => date = next,
            None => break,
        }
    }
    date
}

/// Store open on the next open day after today.
pub fn next_open_morning(now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.with_timezone(&STORE_TZ).date_naive();
    today
        .succ_opt()
        .map(roll_to_open_day)
        .and_then(|d| d.and_hms_opt(super::business_calendar::OPEN_HOUR, 0, 0))
        .and_then(local_to_utc)
        .unwrap_or(now + Duration::hours(16))
}

impl Recurrence {
    /// Validates the stored or requested parts of a rule.
    pub fn from_parts(
        every: &str,
        interval: i64,
        weekdays: &[i64],
        month_day: Option<i64>,
        at: &str,
    ) -> Result<Self, String> {
        let every =
            Every::parse(every).ok_or_else(|| format!("`every` must be once, day, week or month, not `{every}`"))?;
        let at = parse_hhmm(at).ok_or_else(|| format!("`at` must be HH:MM (24-hour), not `{at}`"))?;
        let interval = u32::try_from(interval.max(1)).unwrap_or(1);
        if interval > MAX_INTERVAL {
            return Err(format!("`interval` must be 1..={MAX_INTERVAL}"));
        }
        let mut days = Vec::new();
        for n in weekdays {
            let day = weekday_from_iso(*n).ok_or_else(|| format!("weekday {n} is not 1 (Mon) ..= 7 (Sun)"))?;
            if !days.contains(&day) {
                days.push(day);
            }
        }
        days.sort_by_key(|d| d.number_from_monday());
        match every {
            Every::Day if interval > 1 => {
                return Err("`day` repeats every open day; use `week` with weekdays for other patterns".into());
            }
            Every::Week if days.is_empty() => return Err("`week` needs at least one weekday".into()),
            _ => {}
        }
        let month_day = match (every, month_day) {
            (Every::Month, Some(d)) if (1..=31).contains(&d) => Some(d as u32),
            (Every::Month, Some(d)) => return Err(format!("`month_day` {d} is not 1..=31")),
            (Every::Month, None) => return Err("`month` needs `month_day`".into()),
            _ => None,
        };
        Ok(Self { every, interval, weekdays: days, month_day, at })
    }

    /// The first run strictly after `after`, counting intervals from `anchor`; `None` for `once`.
    pub fn next_after(&self, after: DateTime<Utc>, anchor: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let start = after.with_timezone(&STORE_TZ).date_naive();
        let anchor_date = anchor.with_timezone(&STORE_TZ).date_naive();
        match self.every {
            Every::Once => None,
            Every::Day => (0..14)
                .filter_map(|i| start.checked_add_signed(Duration::days(i)))
                .filter(|d| is_open_day(d.weekday()))
                .filter_map(|d| local_to_utc(d.and_time(self.at)))
                .find(|t| *t > after),
            Every::Week => {
                let span = 7 * i64::from(self.interval) * 2 + 7;
                (0..span)
                    .filter_map(|i| start.checked_add_signed(Duration::days(i)))
                    .filter(|d| self.weekdays.contains(&d.weekday()))
                    .filter(|d| (week_index(*d) - week_index(anchor_date)).rem_euclid(i64::from(self.interval)) == 0)
                    .filter_map(|d| local_to_utc(d.and_time(self.at)))
                    .find(|t| *t > after)
            }
            Every::Month => {
                let wanted = self.month_day?;
                let first = NaiveDate::from_ymd_opt(start.year(), start.month(), 1)?;
                (0..(i64::from(self.interval) * 2 + 2))
                    .filter_map(|i| first.checked_add_months(chrono::Months::new(i as u32)))
                    .filter(|m| (month_index(*m) - month_index(anchor_date)).rem_euclid(i64::from(self.interval)) == 0)
                    .filter_map(|m| {
                        let day = wanted.min(days_in_month(m.year(), m.month()));
                        NaiveDate::from_ymd_opt(m.year(), m.month(), day)
                    })
                    .map(roll_to_open_day)
                    .filter_map(|d| local_to_utc(d.and_time(self.at)))
                    .find(|t| *t > after)
            }
        }
    }

    /// Plain-language summary, e.g. `Mondays and Thursdays at 09:30`.
    pub fn describe(&self) -> String {
        let at = self.at.format("%H:%M");
        match self.every {
            Every::Once => format!("once at {at}"),
            Every::Day => format!("every open day at {at}"),
            Every::Week => {
                let names: Vec<&str> = self.weekdays.iter().map(|d| weekday_plural(*d)).collect();
                let days = join_words(&names);
                if self.interval == 1 {
                    format!("{days} at {at}")
                } else {
                    format!("every {} weeks on {days} at {at}", self.interval)
                }
            }
            Every::Month => {
                let day = self.month_day.unwrap_or(1);
                if self.interval == 1 {
                    format!("the {} of every month at {at}", ordinal(day))
                } else {
                    format!("the {} of every {} months at {at}", ordinal(day), self.interval)
                }
            }
        }
    }
}

fn weekday_plural(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Mondays",
        Weekday::Tue => "Tuesdays",
        Weekday::Wed => "Wednesdays",
        Weekday::Thu => "Thursdays",
        Weekday::Fri => "Fridays",
        Weekday::Sat => "Saturdays",
        Weekday::Sun => "Sundays",
    }
}

fn join_words(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [one] => (*one).to_string(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A clock time: `15:00`, `9:30`, `3pm`, `3:30pm`, `noon`.
pub fn parse_clock(raw: &str) -> Option<NaiveTime> {
    let s = raw.trim().to_ascii_lowercase().replace(' ', "");
    if s == "noon" {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    let (body, pm) = match s.strip_suffix("pm") {
        Some(b) => (b, Some(true)),
        None => match s.strip_suffix("am") {
            Some(b) => (b, Some(false)),
            None => (s.as_str(), None),
        },
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if pm.is_some() => (body.parse::<u32>().ok()?, 0),
        _ => return None,
    };
    let h = match pm {
        Some(true) if (1..=11).contains(&h) => h + 12,
        Some(false) if h == 12 => 0,
        Some(_) if !(1..=12).contains(&h) => return None,
        _ => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

fn parse_offset(s: &str) -> Option<Duration> {
    let s = s.strip_prefix("in ").or_else(|| s.strip_prefix('+'))?.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let n: i64 = s[..split].parse().ok()?;
    match s[split..].trim() {
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(n)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(n)),
        "d" | "day" | "days" => Some(Duration::days(n)),
        _ => None,
    }
}

/// The date a day word names, counted from `today`; weekdays mean the next one, today included.
fn parse_day(word: &str, today: NaiveDate) -> Option<NaiveDate> {
    match word {
        "today" => Some(today),
        "tomorrow" => today.succ_opt(),
        _ => {
            if let Ok(d) = NaiveDate::parse_from_str(word, "%Y-%m-%d") {
                return Some(d);
            }
            let want = weekday_from_name(word).filter(|_| word.parse::<i64>().is_err())?;
            (0..7).filter_map(|i| today.checked_add_signed(Duration::days(i))).find(|d| d.weekday() == want)
        }
    }
}

/// A store-local time phrase as UTC; a day alone takes `default_time`, a past bare clock time the next open day.
pub fn parse_when(raw: &str, now: DateTime<Utc>, default_time: NaiveTime) -> Result<DateTime<Utc>, String> {
    let trimmed = raw.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(t.with_timezone(&Utc));
    }
    let lower = trimmed.to_ascii_lowercase();
    let iso_date = lower.get(..10).is_some_and(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok());
    let lower =
        if iso_date && lower[10..].starts_with('t') { format!("{} {}", &lower[..10], &lower[11..]) } else { lower };
    let s = lower.trim();
    let s = s.strip_prefix("next ").or_else(|| s.strip_prefix("on ")).or_else(|| s.strip_prefix("at ")).unwrap_or(s);
    if s == "now" {
        return Ok(now);
    }
    if let Some(offset) = parse_offset(s) {
        return Ok(now + offset);
    }
    let today = now.with_timezone(&STORE_TZ).date_naive();
    let bad = || {
        format!(
            "could not read `{trimmed}` as a time; use e.g. `tomorrow 15:00`, `friday`, `2026-09-28 09:30` or `in 2 hours`"
        )
    };
    let words: Vec<&str> = s.split_whitespace().collect();
    let (date, time, bare_clock) = match words.as_slice() {
        [] => (today, default_time, false),
        [one] => match (parse_day(one, today), parse_clock(one)) {
            (Some(d), _) => (d, default_time, false),
            (None, Some(t)) => (today, t, true),
            (None, None) => return Err(bad()),
        },
        [day, rest @ ..] => {
            let d = parse_day(day, today).ok_or_else(bad)?;
            let t = parse_clock(&rest.join(" ")).ok_or_else(bad)?;
            (d, t, false)
        }
    };
    let at = local_to_utc(date.and_time(time)).ok_or_else(bad)?;
    if at > now {
        return Ok(at);
    }
    if bare_clock {
        let next = roll_to_open_day(today.succ_opt().ok_or_else(bad)?);
        return local_to_utc(next.and_time(time)).ok_or_else(bad);
    }
    Err(format!(
        "`{trimmed}` is already past (store time is {})",
        now.with_timezone(&STORE_TZ).format("%a %b %-d %H:%M")
    ))
}

/// Store-local rendering used in tool replies, e.g. `Mon Sep 28 10:00`.
pub fn local_label(t: DateTime<Utc>) -> String {
    t.with_timezone(&STORE_TZ).format("%a %b %-d %H:%M").to_string()
}

/// When a task created by a run at `fired` is due: `due_hours` later, else store close that day.
pub fn due_for_run(fired: DateTime<Utc>, due_hours: Option<i64>) -> DateTime<Utc> {
    if let Some(hours) = due_hours {
        return fired + Duration::hours(hours.max(0));
    }
    let local = fired.with_timezone(&STORE_TZ);
    local
        .date_naive()
        .and_hms_opt(CLOSE_HOUR, 0, 0)
        .and_then(local_to_utc)
        .filter(|close| *close > fired + Duration::minutes(59))
        .unwrap_or(fired + Duration::hours(1))
}

/// True once the `task_schedule` table exists; needs INFO rights, so record users get `Err`.
pub const SCHEMA_APPLIED_SQL: &str = "RETURN (INFO FOR DB).tables.task_schedule != NONE";

/// Whether the assistant schema rollout is applied; `Err` when the session may not read INFO.
pub async fn schema_applied() -> anyhow::Result<bool> {
    let applied: Option<bool> = db().query(SCHEMA_APPLIED_SQL).await?.check()?.take(0)?;
    Ok(applied.unwrap_or(false))
}

/// Refusal for scheduling before the rollout is applied.
pub const SCHEMA_MISSING: &str =
    "scheduling is not enabled yet: the ai_assistant_tools schema rollout has not been applied to the database";

/// Deactivates `$id`.
pub const CANCEL_SCHEDULE_SQL: &str = "UPDATE $id SET active = false, next_run = NONE RETURN VALUE id";

/// A schedule to insert; the table defaults fill `runs`, `active` and `created_at`.
#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub struct NewTaskSchedule {
    pub title: String,
    pub description: Option<String>,
    pub assignee: RecordId,
    pub created_by: Option<RecordId>,
    pub every: String,
    pub interval: i64,
    pub weekdays: Vec<i64>,
    pub month_day: Option<i64>,
    pub at: String,
    pub next_run: Option<Datetime>,
    pub due_hours: Option<i64>,
    pub priority: Option<String>,
    pub service_number: Option<String>,
    pub origin: Option<String>,
}

impl NewTaskSchedule {
    pub async fn create(&self) -> anyhow::Result<RecordId> {
        let ids: Vec<RecordId> = db()
            .query("CREATE task_schedule CONTENT $row RETURN VALUE id")
            .bind(("row", self.clone()))
            .await?
            .check()?
            .take(0)?;
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("schedule create returned no id"))
    }
}

/// One `task_schedule` row.
#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub struct TaskSchedule {
    pub id: RecordId,
    pub title: String,
    pub description: Option<String>,
    pub assignee: RecordId,
    pub created_by: Option<RecordId>,
    pub every: String,
    #[surreal(default)]
    pub interval: i64,
    #[surreal(default)]
    pub weekdays: Vec<i64>,
    pub month_day: Option<i64>,
    pub at: String,
    pub next_run: Option<Datetime>,
    pub last_run: Option<Datetime>,
    #[surreal(default)]
    pub runs: i64,
    #[surreal(default)]
    pub active: bool,
    pub due_hours: Option<i64>,
    pub priority: Option<String>,
    pub service_number: Option<String>,
    pub origin: Option<String>,
    pub created_at: Option<Datetime>,
}

impl TaskSchedule {
    pub fn recurrence(&self) -> Result<Recurrence, String> {
        Recurrence::from_parts(&self.every, self.interval, &self.weekdays, self.month_day, &self.at)
    }

    /// Plain-language timing, e.g. `Mondays at 10:00`.
    pub fn describe(&self) -> String {
        match (self.recurrence(), &self.next_run) {
            (Ok(r), Some(next)) if r.every == Every::Once => {
                format!("once on {}", next.into_inner().with_timezone(&STORE_TZ).format("%a %b %-d at %H:%M"))
            }
            (Ok(r), _) => r.describe(),
            (Err(_), _) => format!("every {} at {}", self.every, self.at),
        }
    }

    /// The run after the one at `fired`; `None` when the schedule is spent.
    pub fn following_run(&self, fired: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let anchor = self.created_at.map(Datetime::into_inner).unwrap_or(fired);
        self.recurrence().ok()?.next_after(fired, anchor)
    }

    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let rows: Vec<Self> = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?.check()?.take(0)?;
        Ok(rows.into_iter().next())
    }

    /// Active schedules assigned to or created by `user`.
    pub async fn for_user(user: &RecordId) -> anyhow::Result<Vec<Self>> {
        Ok(db().query(SCHEDULES_FOR_USER_SQL).bind(("user", user.clone())).await?.check()?.take(0)?)
    }

    /// Stops the schedule; true when a row was changed.
    pub async fn cancel(id: &RecordId) -> anyhow::Result<bool> {
        let ids: Vec<RecordId> = db().query(CANCEL_SCHEDULE_SQL).bind(("id", id.clone())).await?.check()?.take(0)?;
        Ok(!ids.is_empty())
    }

    /// Due schedules, oldest first.
    pub async fn due(limit: i64) -> anyhow::Result<Vec<Self>> {
        Ok(db().query(DUE_SCHEDULES_SQL).bind(("limit", limit)).await?.check()?.take(0)?)
    }

    /// Advances the schedule past `expected`; false when another worker already took the run.
    pub async fn claim_run(&self, expected: &Datetime, fired: DateTime<Utc>) -> anyhow::Result<bool> {
        let next = self.following_run(fired);
        let won: Vec<RecordId> = db()
            .query(CLAIM_RUN_SQL)
            .bind(("id", self.id.clone()))
            .bind(("expected", *expected))
            .bind(("next", next.map(Datetime::from)))
            .bind(("fired", Datetime::from(fired)))
            .bind(("active", next.is_some()))
            .await?
            .check()?
            .take(0)?;
        Ok(!won.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn local(dt: DateTime<Utc>) -> String {
        dt.with_timezone(&STORE_TZ).format("%a %Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn weekly_monday_rolls_to_next_week_after_the_slot() {
        let r = Recurrence::from_parts("week", 1, &[1], None, "10:00").unwrap();
        // Mon 2026-09-28 11:00 local, after that day's slot.
        let next = r.next_after(utc("2026-09-28T17:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(next), "Mon 2026-10-05 10:00");
    }

    #[test]
    fn weekly_same_day_before_the_slot_fires_today() {
        let r = Recurrence::from_parts("week", 1, &[1, 4], None, "10:00").unwrap();
        // Mon 2026-09-28 09:00 local.
        let next = r.next_after(utc("2026-09-28T15:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(next), "Mon 2026-09-28 10:00");
    }

    #[test]
    fn biweekly_counts_weeks_from_the_anchor() {
        let r = Recurrence::from_parts("week", 2, &[3], None, "09:30").unwrap();
        // Anchor week of Mon 2026-09-28; after Wed 2026-09-30 10:00 the next is two weeks on.
        let anchor = utc("2026-09-28T16:00:00Z");
        let next = r.next_after(utc("2026-09-30T16:00:00Z"), anchor).unwrap();
        assert_eq!(local(next), "Wed 2026-10-14 09:30");
    }

    #[test]
    fn daily_skips_sunday() {
        let r = Recurrence::from_parts("day", 1, &[], None, "10:00").unwrap();
        // Sat 2026-10-03 12:00 local.
        let next = r.next_after(utc("2026-10-03T18:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(next), "Mon 2026-10-05 10:00");
    }

    #[test]
    fn monthly_clamps_to_month_end_and_rolls_off_sunday() {
        let r = Recurrence::from_parts("month", 1, &[], Some(31), "10:00").unwrap();
        // After Oct 31 2026 (a Saturday): November has 30 days, Nov 30 2026 is a Monday.
        let next = r.next_after(utc("2026-10-31T17:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(next), "Mon 2026-11-30 10:00");
        // Jan 31 2027 is a Sunday, so that run moves to Monday Feb 1.
        let jan = r.next_after(utc("2027-01-20T00:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(jan), "Mon 2027-02-01 10:00");
        // Feb 28 2027 is a Sunday, so that run moves to Monday Mar 1.
        let feb = r.next_after(utc("2027-02-10T00:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(feb), "Mon 2027-03-01 10:00");
    }

    #[test]
    fn quarterly_skips_off_months() {
        let r = Recurrence::from_parts("month", 3, &[], Some(1), "10:00").unwrap();
        let anchor = utc("2026-09-15T16:00:00Z");
        let next = r.next_after(utc("2026-09-15T16:00:00Z"), anchor).unwrap();
        // Dec 1 2026 is a Tuesday.
        assert_eq!(local(next), "Tue 2026-12-01 10:00");
    }

    #[test]
    fn dst_change_keeps_local_wall_time() {
        let r = Recurrence::from_parts("week", 1, &[1], None, "10:00").unwrap();
        // DST ends Sun 2026-11-01; Mondays either side stay 10:00 local.
        let before = r.next_after(utc("2026-10-25T00:00:00Z"), utc("2026-09-01T00:00:00Z")).unwrap();
        let after = r.next_after(before, utc("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(local(before), "Mon 2026-10-26 10:00");
        assert_eq!(local(after), "Mon 2026-11-02 10:00");
        assert_ne!((after - before).num_hours(), 168);
    }

    #[test]
    fn invalid_rules_are_rejected() {
        assert!(Recurrence::from_parts("hourly", 1, &[], None, "10:00").is_err());
        assert!(Recurrence::from_parts("week", 1, &[], None, "10:00").is_err());
        assert!(Recurrence::from_parts("week", 1, &[8], None, "10:00").is_err());
        assert!(Recurrence::from_parts("day", 2, &[], None, "10:00").is_err());
        assert!(Recurrence::from_parts("month", 1, &[], None, "10:00").is_err());
        assert!(Recurrence::from_parts("day", 1, &[], None, "25:00").is_err());
        assert!(Recurrence::from_parts("day", 1, &[], None, "9:00").is_err());
    }

    #[test]
    fn once_has_no_following_run() {
        let r = Recurrence::from_parts("once", 1, &[], None, "10:00").unwrap();
        assert_eq!(r.next_after(utc("2026-09-28T15:00:00Z"), utc("2026-09-01T00:00:00Z")), None);
    }

    #[test]
    fn describe_reads_naturally() {
        let r = Recurrence::from_parts("week", 1, &[4, 1], None, "09:30").unwrap();
        assert_eq!(r.describe(), "Mondays and Thursdays at 09:30");
        let m = Recurrence::from_parts("month", 1, &[], Some(2), "10:00").unwrap();
        assert_eq!(m.describe(), "the 2nd of every month at 10:00");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(23), "23rd");
    }

    #[test]
    fn weekday_names_parse() {
        assert_eq!(weekday_from_name("Monday"), Some(Weekday::Mon));
        assert_eq!(weekday_from_name("Mondays"), Some(Weekday::Mon));
        assert_eq!(weekday_from_name("thu"), Some(Weekday::Thu));
        assert_eq!(weekday_from_name("7"), Some(Weekday::Sun));
        assert_eq!(weekday_from_name("month"), None);
        assert_eq!(weekday_from_name("x"), None);
    }

    #[test]
    fn clock_times_parse() {
        assert_eq!(parse_clock("15:00"), NaiveTime::from_hms_opt(15, 0, 0));
        assert_eq!(parse_clock("9:30"), NaiveTime::from_hms_opt(9, 30, 0));
        assert_eq!(parse_clock("3pm"), NaiveTime::from_hms_opt(15, 0, 0));
        assert_eq!(parse_clock("3:30 PM"), NaiveTime::from_hms_opt(15, 30, 0));
        assert_eq!(parse_clock("12am"), NaiveTime::from_hms_opt(0, 0, 0));
        assert_eq!(parse_clock("noon"), NaiveTime::from_hms_opt(12, 0, 0));
        assert_eq!(parse_clock("15"), None);
        assert_eq!(parse_clock("13pm"), None);
    }

    #[test]
    fn when_phrases_resolve_in_store_time() {
        // Sat 2026-09-26 11:00 local.
        let now = utc("2026-09-26T17:00:00Z");
        let close = NaiveTime::from_hms_opt(19, 0, 0).unwrap();
        let t = |raw: &str| parse_when(raw, now, close).map(local);
        assert_eq!(t("today").unwrap(), "Sat 2026-09-26 19:00");
        assert_eq!(t("tomorrow 3pm").unwrap(), "Sun 2026-09-27 15:00");
        assert_eq!(t("monday").unwrap(), "Mon 2026-09-28 19:00");
        assert_eq!(t("next friday 09:30").unwrap(), "Fri 2026-10-02 09:30");
        assert_eq!(t("2026-09-29 08:15").unwrap(), "Tue 2026-09-29 08:15");
        assert_eq!(t("2026-09-29T08:15").unwrap(), "Tue 2026-09-29 08:15");
        assert_eq!(t("in 2 hours").unwrap(), "Sat 2026-09-26 13:00");
        assert_eq!(t("+30m").unwrap(), "Sat 2026-09-26 11:30");
        // A bare clock time already past moves to the next open day.
        assert_eq!(t("10:00").unwrap(), "Mon 2026-09-28 10:00");
        assert_eq!(t("16:00").unwrap(), "Sat 2026-09-26 16:00");
        assert!(t("2026-09-01").unwrap_err().contains("already past"));
        assert!(t("someday").is_err());
        assert_eq!(parse_when("2026-09-28T16:00:00Z", now, close).unwrap(), utc("2026-09-28T16:00:00Z"));
    }

    #[test]
    fn due_defaults_to_store_close() {
        // Mon 10:00 local run is due at 19:00 local the same day.
        let due = due_for_run(utc("2026-09-28T16:00:00Z"), None);
        assert_eq!(local(due), "Mon 2026-09-28 19:00");
        // A run after close is due an hour later.
        let late = due_for_run(utc("2026-09-29T02:00:00Z"), None);
        assert_eq!(late, utc("2026-09-29T03:00:00Z"));
        assert_eq!(due_for_run(utc("2026-09-28T16:00:00Z"), Some(48)), utc("2026-09-30T16:00:00Z"));
    }

    #[test]
    fn partial_row_deserializes() {
        let row = TaskSchedule {
            id: RecordId::new("task_schedule", "a"),
            title: "Count paste".into(),
            description: None,
            assignee: RecordId::new("user", "sam"),
            created_by: None,
            every: "week".into(),
            interval: 1,
            weekdays: vec![1],
            month_day: None,
            at: "10:00".into(),
            next_run: None,
            last_run: None,
            runs: 0,
            active: true,
            due_hours: None,
            priority: None,
            service_number: None,
            origin: None,
            created_at: None,
        };
        let mut value = row.into_value();
        if let surrealdb::types::Value::Object(obj) = &mut value {
            for key in ["interval", "weekdays", "runs", "active", "description", "created_by"] {
                obj.remove(key);
            }
        }
        let back = TaskSchedule::from_value(value).expect("partial row deserializes");
        assert_eq!(back.title, "Count paste");
        assert!(back.weekdays.is_empty());
    }
}

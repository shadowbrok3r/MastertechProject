//! Store-calendar arithmetic: PC Laptops stores open 10:00-19:00 local,
//! Monday through Saturday.
//!
//! Wall-clock spans overstate service time because they count nights, Sundays
//! and holidays as if work were possible. Comeback windows stay in calendar
//! days - a customer's machine fails on customer time - but anything measuring
//! shop effort or shop-side paperwork belongs on this calendar.

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc, Weekday};
use chrono_tz::America::Denver;
use chrono_tz::Tz;

pub const STORE_TZ: Tz = Denver;
pub const OPEN_HOUR: u32 = 10;
pub const CLOSE_HOUR: u32 = 19;
/// Seconds a store is open on a normal day.
pub const OPEN_SECS_PER_DAY: i64 = (CLOSE_HOUR - OPEN_HOUR) as i64 * 3600;

fn is_open_day(day: Weekday) -> bool {
    day != Weekday::Sun
}

/// Seconds of open-store time between two instants; 0 when end precedes start.
pub fn business_seconds(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    if end <= start {
        return 0;
    }
    let (start_local, end_local) = (start.with_timezone(&STORE_TZ), end.with_timezone(&STORE_TZ));
    let mut total = 0i64;
    let mut day = start_local.date_naive();
    let last = end_local.date_naive();
    while day <= last {
        if is_open_day(day.weekday()) {
            let open = day
                .and_hms_opt(OPEN_HOUR, 0, 0)
                .and_then(|t| STORE_TZ.from_local_datetime(&t).single());
            let close = day
                .and_hms_opt(CLOSE_HOUR, 0, 0)
                .and_then(|t| STORE_TZ.from_local_datetime(&t).single());
            if let (Some(open), Some(close)) = (open, close) {
                let from = open.max(start_local);
                let to = close.min(end_local);
                if to > from {
                    total += (to - from).num_seconds();
                }
            }
        }
        match day.succ_opt() {
            Some(next) => day = next,
            None => break,
        }
    }
    total
}

/// Open days elapsed between two instants, counting the days the shop was
/// actually open (Sundays skipped). Whole days, truncated.
pub fn open_days_between(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    if end <= start {
        return 0;
    }
    let mut day = start.with_timezone(&STORE_TZ).date_naive();
    let last = end.with_timezone(&STORE_TZ).date_naive();
    let mut open = 0i64;
    while day < last {
        match day.succ_opt() {
            Some(next) => day = next,
            None => break,
        }
        if is_open_day(day.weekday()) {
            open += 1;
        }
    }
    open
}

/// Shifts an instant by whole open days, keeping the time of day and skipping
/// closed days. Negative counts walk backwards.
pub fn add_business_days(at: DateTime<Utc>, days: i64) -> DateTime<Utc> {
    let step = if days < 0 { -1 } else { 1 };
    let mut remaining = days.abs();
    let mut cursor = at;
    while remaining > 0 {
        cursor += Duration::days(step);
        if is_open_day(cursor.with_timezone(&STORE_TZ).weekday()) {
            remaining -= 1;
        }
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn overnight_counts_only_open_hours() {
        // Mon 17:00 -> Tue 11:00 local = 2 open hours Monday + 1 Tuesday.
        let secs = business_seconds(utc("2026-08-03T23:00:00Z"), utc("2026-08-04T17:00:00Z"));
        assert_eq!(secs, 3 * 3600);
    }

    #[test]
    fn sunday_contributes_nothing() {
        // Sat 19:00 -> Mon 10:00 local spans a full Sunday and two closures.
        let secs = business_seconds(utc("2026-08-09T01:00:00Z"), utc("2026-08-10T16:00:00Z"));
        assert_eq!(secs, 0);
    }

    #[test]
    fn full_open_day_is_nine_hours() {
        let secs = business_seconds(utc("2026-08-04T16:00:00Z"), utc("2026-08-05T01:00:00Z"));
        assert_eq!(secs, OPEN_SECS_PER_DAY);
    }

    #[test]
    fn week_of_wall_clock_is_six_open_days() {
        // Mon -> Mon spans one Sunday.
        assert_eq!(open_days_between(utc("2026-08-03T23:05:00Z"), utc("2026-08-10T16:54:00Z")), 6);
    }

    #[test]
    fn business_days_skip_closures() {
        // Sat 12:00 local + 1 open day lands Monday, not Sunday.
        let sat = utc("2026-08-08T18:00:00Z");
        let next = add_business_days(sat, 1);
        assert_eq!(next.with_timezone(&STORE_TZ).weekday(), Weekday::Mon);
        // Three open days back from Tuesday reaches the previous Friday.
        let tue = utc("2026-08-18T18:00:00Z");
        let back = add_business_days(tue, -3);
        assert_eq!(back.with_timezone(&STORE_TZ).weekday(), Weekday::Fri);
    }

    #[test]
    fn zero_and_reversed_spans_are_zero() {
        let a = utc("2026-08-04T18:00:00Z");
        assert_eq!(business_seconds(a, a), 0);
        assert_eq!(business_seconds(a, a - Duration::hours(5)), 0);
        assert_eq!(open_days_between(a, a), 0);
    }
}

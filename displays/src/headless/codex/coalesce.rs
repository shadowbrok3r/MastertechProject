//! Streamed transcript text and thread-row fields held in memory between database writes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use database::schema::agent_thread::AgentThreadState;

/// Shortest gap between two writes of one streaming item.
pub const ITEM_FLUSH: Duration = Duration::from_secs(1);
/// Shortest gap between two token-count writes of the thread row.
pub const THREAD_FLUSH: Duration = Duration::from_secs(5);

/// The current text of an item still in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partial {
    pub item_id: String,
    pub seq: i64,
    pub kind: &'static str,
    pub turn: Option<String>,
    pub text: String,
}

#[derive(Debug)]
struct Pending {
    seq: i64,
    kind: &'static str,
    turn: Option<String>,
    text: String,
    dirty: bool,
    written: bool,
    /// Time of the last write, or of first sight while unwritten.
    since: Instant,
}

/// Per-item text released at most once per interval.
#[derive(Debug)]
pub struct TranscriptBuffer {
    items: HashMap<String, Pending>,
    interval: Duration,
}

impl TranscriptBuffer {
    pub fn new(interval: Duration) -> Self {
        Self { items: HashMap::new(), interval }
    }

    pub fn contains(&self, item_id: &str) -> bool {
        self.items.contains_key(item_id)
    }

    /// Starts buffering an item; its first write waits for the interval or for completion.
    pub fn open(
        &mut self,
        item_id: &str,
        seq: i64,
        kind: &'static str,
        turn: Option<String>,
        text: String,
        now: Instant,
    ) {
        self.items.entry(item_id.to_string()).or_insert(Pending {
            seq,
            kind,
            turn,
            text,
            dirty: true,
            written: false,
            since: now,
        });
    }

    /// Appends streamed text without writing it.
    pub fn append(&mut self, item_id: &str, delta: &str) {
        if let Some(pending) = self.items.get_mut(item_id) {
            pending.text.push_str(delta);
            pending.dirty = true;
        }
    }

    /// Appends streamed text and returns a write once the item's interval has passed.
    pub fn push(&mut self, item_id: &str, delta: &str, now: Instant) -> Option<Partial> {
        self.append(item_id, delta);
        let interval = self.interval;
        let pending = self.items.get_mut(item_id)?;
        (now.duration_since(pending.since) >= interval).then(|| take(item_id, pending, now))
    }

    /// Writes for changed items whose interval has passed, lowest seq first.
    pub fn due(&mut self, now: Instant) -> Vec<Partial> {
        let interval = self.interval;
        self.collect(now, |p| now.duration_since(p.since) >= interval)
    }

    /// Writes for every changed item, lowest seq first.
    pub fn drain(&mut self, now: Instant) -> Vec<Partial> {
        self.collect(now, |_| true)
    }

    /// Writes for never-written items that sort before `seq`.
    pub fn unwritten_before(&mut self, seq: i64, now: Instant) -> Vec<Partial> {
        self.collect(now, |p| !p.written && p.seq < seq)
    }

    /// Stops buffering an item whose authoritative write follows.
    pub fn close(&mut self, item_id: &str) {
        self.items.remove(item_id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    fn collect(&mut self, now: Instant, want: impl Fn(&Pending) -> bool) -> Vec<Partial> {
        let mut out: Vec<Partial> = self
            .items
            .iter_mut()
            .filter(|(_, p)| p.dirty && want(p))
            .map(|(id, p)| take(id, p, now))
            .collect();
        out.sort_by_key(|p| p.seq);
        out
    }
}

fn take(item_id: &str, pending: &mut Pending, now: Instant) -> Partial {
    pending.dirty = false;
    pending.written = true;
    pending.since = now;
    Partial {
        item_id: item_id.to_string(),
        seq: pending.seq,
        kind: pending.kind,
        turn: pending.turn.clone(),
        text: pending.text.clone(),
    }
}

/// Thread-row fields the runner keeps current, and what it last wrote.
#[derive(Debug)]
pub struct ThreadRow {
    status: Option<String>,
    seq: i64,
    written_seq: i64,
    tokens: (Option<i64>, Option<i64>),
    written_tokens: (Option<i64>, Option<i64>),
    tokens_at: Option<Instant>,
    interval: Duration,
}

impl ThreadRow {
    /// Starts from the row as read, with no status recorded as written.
    pub fn new(seq: i64, tokens: (Option<i64>, Option<i64>), interval: Duration) -> Self {
        Self {
            status: None,
            seq,
            written_seq: seq,
            tokens,
            written_tokens: tokens,
            tokens_at: None,
            interval,
        }
    }

    pub fn touch(&mut self, seq: i64) {
        self.seq = self.seq.max(seq);
    }

    /// Forgets the written status, forcing the next status write.
    pub fn forget_status(&mut self) {
        self.status = None;
    }

    /// Keeps the latest counts; a missing value leaves the previous one.
    pub fn set_tokens(&mut self, used: Option<i64>, window: Option<i64>) {
        self.tokens = (used.or(self.tokens.0), window.or(self.tokens.1));
    }

    /// The write a status change needs; `None` when the status is unchanged and carries no error.
    pub fn status(&mut self, status: &str, error: Option<&str>, now: Instant) -> Option<AgentThreadState> {
        if error.is_none() && self.status.as_deref() == Some(status) {
            return None;
        }
        self.status = Some(status.to_string());
        let mut state = self.pending(now);
        state.status = Some(status.to_string());
        state.error = error.map(str::to_string);
        Some(state)
    }

    /// The write changed token counts need once the interval has passed.
    pub fn due(&mut self, now: Instant) -> Option<AgentThreadState> {
        let changed = self.tokens != self.written_tokens;
        let waited = self.tokens_at.is_none_or(|t| now.duration_since(t) >= self.interval);
        (changed && waited).then(|| self.pending(now))
    }

    /// The write every pending change needs, due or not.
    pub fn drain(&mut self, now: Instant) -> Option<AgentThreadState> {
        let state = self.pending(now);
        (!state.is_empty()).then_some(state)
    }

    /// Pending cursor and token changes, marked written.
    fn pending(&mut self, now: Instant) -> AgentThreadState {
        let mut state = AgentThreadState::default();
        if self.seq > self.written_seq {
            state.last_seq = Some(self.seq);
            self.written_seq = self.seq;
        }
        if self.tokens != self.written_tokens {
            (state.tokens_used, state.tokens_window) = self.tokens;
            self.written_tokens = self.tokens;
            self.tokens_at = Some(now);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(250);

    /// Writes for one item fed a delta every `gap` for `secs`, ticked like the runner, then completed.
    fn stream(buf: &mut TranscriptBuffer, id: &str, seq: i64, t0: Instant, gap: Duration, secs: u64) -> (usize, Instant) {
        buf.open(id, seq, "agent", None, String::new(), t0);
        let end = t0 + Duration::from_secs(secs);
        let (mut writes, mut now, mut next_tick) = (0, t0, t0 + TICK);
        while now < end {
            now += gap;
            while next_tick <= now {
                writes += buf.due(next_tick).len();
                next_tick += TICK;
            }
            writes += usize::from(buf.push(id, "tok ", now).is_some());
        }
        buf.close(id);
        (writes + 1, now)
    }

    #[test]
    fn a_streamed_reply_writes_about_once_a_second_instead_of_once_per_delta() {
        let t0 = Instant::now();
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        let raw_deltas = 20_000 / 33;
        let (writes, _) = stream(&mut buf, "a", 1, t0, Duration::from_millis(33), 20);
        assert!(writes <= 21, "{writes} writes for a 20 s reply");
        assert!(writes >= 18, "partial text must still land about every second: {writes}");
        assert!(writes * 25 < raw_deltas, "{writes} writes vs {raw_deltas} deltas");
    }

    #[test]
    fn chunked_input_keeps_the_one_second_cadence() {
        let t0 = Instant::now();
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        let (writes, _) = stream(&mut buf, "r", 1, t0, Duration::from_millis(900), 10);
        assert!((9..=11).contains(&writes), "{writes} writes for a 10 s item fed 900 ms chunks");
    }

    #[test]
    fn an_item_that_completes_inside_the_interval_is_written_once() {
        let t0 = Instant::now();
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        buf.open("tool", 3, "tool_call", None, "scripts_list({})".into(), t0);
        assert!(buf.due(t0 + Duration::from_millis(500)).is_empty());
        buf.close("tool");
        assert!(buf.drain(t0 + Duration::from_secs(5)).is_empty());
    }

    #[test]
    fn a_slow_item_becomes_visible_after_one_interval() {
        let t0 = Instant::now();
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        buf.open("tool", 3, "tool_call", None, "scripts_run_remote(..)".into(), t0);
        let due = buf.due(t0 + ITEM_FLUSH);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].text, "scripts_run_remote(..)");
        assert!(buf.due(t0 + ITEM_FLUSH * 5).is_empty(), "an unchanged item is not rewritten");
    }

    #[test]
    fn drain_releases_every_change_regardless_of_time() {
        let t0 = Instant::now();
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        buf.open("b", 5, "agent", None, String::new(), t0);
        buf.open("a", 4, "reasoning", None, String::new(), t0);
        assert!(buf.push("a", "thinking", t0).is_none());
        assert!(buf.push("b", "hello", t0).is_none());
        let out = buf.drain(t0);
        assert_eq!(out.iter().map(|p| p.seq).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(out[1].text, "hello");
        assert!(buf.drain(t0).is_empty(), "nothing is written twice");
    }

    #[test]
    fn unwritten_items_before_a_row_are_written_first() {
        let t0 = Instant::now();
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        buf.open("early", 7, "tool_call", None, "a".into(), t0);
        buf.open("later", 9, "tool_call", None, "b".into(), t0);
        let out = buf.unwritten_before(8, t0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].item_id, "early");
        assert!(buf.unwritten_before(8, t0).is_empty());
    }

    #[test]
    fn thread_status_is_written_only_when_it_changes() {
        let t0 = Instant::now();
        let mut row = ThreadRow::new(10, (None, Some(1000)), THREAD_FLUSH);
        assert!(row.status("running", None, t0).is_some(), "the first status is always written");
        assert!(row.status("running", None, t0).is_none());
        assert!(row.status("running", Some("boom"), t0).is_some(), "an error is always written");
        assert!(row.status("idle", None, t0).is_some());
    }

    #[test]
    fn a_status_write_carries_the_pending_cursor_and_tokens() {
        let t0 = Instant::now();
        let mut row = ThreadRow::new(10, (None, None), THREAD_FLUSH);
        row.touch(14);
        row.set_tokens(Some(4000), Some(1000));
        let state = row.status("idle", None, t0).expect("a change");
        assert_eq!(state.last_seq, Some(14));
        assert_eq!((state.tokens_used, state.tokens_window), (Some(4000), Some(1000)));
        assert!(row.drain(t0).is_none(), "nothing left to write");
    }

    #[test]
    fn token_counts_are_written_when_they_change_and_at_most_every_interval() {
        let t0 = Instant::now();
        let mut row = ThreadRow::new(0, (Some(100), Some(1000)), THREAD_FLUSH);
        row.set_tokens(Some(100), Some(1000));
        assert!(row.due(t0).is_none(), "unchanged counts are not written");
        let mut writes = 0;
        for i in 0..60u64 {
            let now = t0 + Duration::from_secs(i);
            row.set_tokens(Some(100 + i as i64), None);
            writes += usize::from(row.due(now).is_some());
        }
        assert!((12..=13).contains(&writes), "{writes} token writes in 60 s of per-second updates");
        assert_eq!(row.drain(t0 + Duration::from_secs(61)).and_then(|s| s.tokens_used), Some(159));
    }

    /// One response at the session's medians: 10 s reasoning, a 4 s reply, a quick tool call, one token update.
    #[test]
    fn a_typical_response_costs_well_under_the_old_write_count() {
        let t0 = Instant::now();
        let chunk = Duration::from_millis(900);
        let mut buf = TranscriptBuffer::new(ITEM_FLUSH);
        let mut row = ThreadRow::new(0, (Some(1), Some(1000)), THREAD_FLUSH);
        row.status("running", None, t0);

        let (reasoning, t1) = stream(&mut buf, "r", 1, t0, chunk, 10);
        let (reply, t2) = stream(&mut buf, "a", 2, t1, chunk, 4);
        buf.open("t", 3, "tool_call", None, "scripts_list({})".into(), t2);
        buf.close("t");
        let tool = 1;
        row.touch(3);
        row.set_tokens(Some(2), None);
        let thread = usize::from(row.due(t2).is_some());
        let new = reasoning + reply + tool + thread;

        // Started write, seq touch, timer chunks, final chunk and completion per streamed item.
        let per_item = |d: Duration| 2 + (d.as_secs_f64() / 0.9).floor() as usize + 2;
        let old = per_item(t1 - t0) + per_item(t2 - t1) + 3 + 1;
        println!("typical response: {old} writes before, {new} after");
        assert!(new * 10 <= old * 7, "{new} writes after vs {old} before");
    }
}

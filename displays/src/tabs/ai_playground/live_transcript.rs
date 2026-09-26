//! The open agent session's transcript, streamed from `agent_event`: a LIVE SELECT pushes new and
//! changing rows, including rows still streaming, and a slow snapshot fills any gap a dropped
//! stream leaves.

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::live_data::{Action, listen_data_filtered};
use database::schema::{AgentEvent, RecordId, RecordIdExt};
use eframe::egui::Context;
use futures::future::AbortHandle;
use web_time::Instant;

use crate::{PlatformSpawner, Spawner};

/// Snapshot of rows after the newest one held, to catch up after a dropped stream.
const SNAPSHOT_POLL: Duration = Duration::from_secs(10);
const STREAM_RETRY: Duration = Duration::from_secs(3);
/// Newest rows read when a session is first opened.
const FIRST_PAGE: usize = 400;
/// Rows read per catch-up snapshot.
const CATCH_UP_PAGE: usize = 400;
/// Repaint cadence while a row is still streaming.
const STREAMING_TICK: Duration = Duration::from_millis(400);
const IDLE_TICK: Duration = Duration::from_secs(2);

enum Msg {
    Events(RecordId, Result<Vec<AgentEvent>, String>),
    Ended(u64, Option<String>),
}

/// One abortable LIVE SELECT and when to reopen it after it drops.
#[derive(Default)]
struct LiveStream {
    generation: u64,
    abort: Option<AbortHandle>,
    retry_at: Option<Instant>,
}

impl LiveStream {
    fn stop(&mut self) {
        if let Some(handle) = self.abort.take() {
            handle.abort();
        }
        self.retry_at = None;
    }

    fn due(&self) -> bool {
        self.abort.is_none() && self.retry_at.is_none_or(|t| Instant::now() >= t)
    }

    /// Marks the stream closed; `false` when the notice belongs to a superseded generation.
    fn ended(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.abort = None;
        self.retry_at = Some(Instant::now() + STREAM_RETRY);
        true
    }
}

pub struct LiveTranscript {
    thread: Option<RecordId>,
    events: Vec<AgentEvent>,
    last_seq: i64,
    /// Set once the first snapshot of the followed session has arrived.
    loaded: bool,
    stream: LiveStream,
    live_tx: Sender<(Action, AgentEvent)>,
    live_rx: Receiver<(Action, AgentEvent)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    loading: bool,
    last_snapshot: Option<Instant>,
    error: Option<String>,
}

impl Default for LiveTranscript {
    fn default() -> Self {
        let (live_tx, live_rx) = unbounded();
        let (tx, rx) = unbounded();
        Self {
            thread: None,
            events: Vec::new(),
            last_seq: 0,
            loaded: false,
            stream: LiveStream::default(),
            live_tx,
            live_rx,
            tx,
            rx,
            loading: false,
            last_snapshot: None,
            error: None,
        }
    }
}

impl LiveTranscript {
    /// Follows `thread`, or nothing; a different thread starts empty.
    pub fn follow(&mut self, thread: Option<RecordId>) {
        if self.thread == thread {
            return;
        }
        self.stream.stop();
        self.thread = thread;
        self.events.clear();
        self.last_seq = 0;
        self.loaded = false;
        self.last_snapshot = None;
        self.error = None;
    }

    /// The followed session.
    pub fn thread(&self) -> Option<&RecordId> {
        self.thread.as_ref()
    }

    /// The followed session's rows in `seq` order.
    pub fn events(&self) -> &[AgentEvent] {
        &self.events
    }

    /// Whether the followed session's first snapshot has arrived.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// Whether the stream is open for the followed session.
    pub fn streaming(&self) -> bool {
        self.stream.abort.is_some()
    }

    /// The last stream or snapshot error, if the transcript may be behind.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Applies streamed and snapshot rows, keeps the stream open and the snapshot running; call every frame.
    pub fn tick(&mut self, ctx: &Context) {
        self.drain();
        let Some(thread) = self.thread.clone() else { return };
        if self.stream.due() {
            self.start_stream(&thread);
        }
        if !self.loading && self.last_snapshot.is_none_or(|t| t.elapsed() >= SNAPSHOT_POLL) {
            self.snapshot(thread);
        }
        let streaming = self.events.iter().any(|e| !e.done);
        ctx.request_repaint_after(if streaming { STREAMING_TICK } else { IDLE_TICK });
    }

    fn drain(&mut self) {
        while let Ok((action, row)) = self.live_rx.try_recv() {
            if self.thread.as_ref() != Some(&row.thread) {
                continue;
            }
            match action {
                Action::Delete => self.events.retain(|e| e.id != row.id),
                Action::Create | Action::Update => self.merge(row),
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Events(thread, result) => {
                    self.loading = false;
                    if self.thread.as_ref() != Some(&thread) {
                        continue;
                    }
                    match result {
                        Ok(rows) => {
                            for row in rows {
                                self.merge(row);
                            }
                            self.loaded = true;
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Msg::Ended(generation, error) => {
                    if self.stream.ended(generation) {
                        self.last_snapshot = None;
                        if let Some(e) = error {
                            self.error = Some(format!("transcript stream dropped, reconnecting: {e}"));
                        }
                    }
                }
            }
        }
    }

    fn merge(&mut self, row: AgentEvent) {
        self.last_seq = self.last_seq.max(row.seq);
        merge_event(&mut self.events, row);
    }

    fn start_stream(&mut self, thread: &RecordId) {
        self.stream.stop();
        let key = thread.key_string();
        // The key is inlined, so only the alphabet SurrealDB generates is accepted.
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
            self.stream.retry_at = Some(Instant::now() + Duration::from_secs(3600));
            return;
        }
        self.stream.generation += 1;
        let generation = self.stream.generation;
        let query = format!("LIVE SELECT * FROM agent_event WHERE thread = agent_thread:`{key}`");
        let live_tx = self.live_tx.clone();
        let tx = self.tx.clone();
        let (fut, handle) = futures::future::abortable(async move {
            let res = listen_data_filtered::<AgentEvent>(live_tx, query, Vec::new(), None).await;
            let _ = tx.send(Msg::Ended(generation, res.err().map(|e| e.to_string())));
        });
        self.stream.abort = Some(handle);
        PlatformSpawner::spawn(async move {
            let _ = fut.await;
        });
    }

    fn snapshot(&mut self, thread: RecordId) {
        self.loading = true;
        self.last_snapshot = Some(Instant::now());
        let tx = self.tx.clone();
        let first = !self.loaded;
        let after = self.last_seq;
        PlatformSpawner::spawn(async move {
            let rows = if first {
                AgentEvent::recent(&thread, FIRST_PAGE).await
            } else {
                AgentEvent::since(&thread, after, CATCH_UP_PAGE).await
            };
            let _ = tx.send(Msg::Events(thread, rows.map_err(|e| e.to_string())));
        });
    }
}

/// Replaces the row with `row`'s id or adds it, keeping `seq` order.
fn merge_event(events: &mut Vec<AgentEvent>, row: AgentEvent) {
    match events.iter_mut().find(|e| e.id == row.id) {
        Some(existing) => *existing = row,
        None => events.push(row),
    }
    events.sort_by_key(|e| e.seq);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, seq: i64, text: &str, done: bool) -> AgentEvent {
        AgentEvent {
            id: RecordId::new("agent_event", id),
            thread: RecordId::new("agent_thread", "t"),
            seq,
            turn_id: None,
            item_id: None,
            kind: "agent".into(),
            text: text.into(),
            done,
            item: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn a_streaming_row_is_replaced_in_place_and_rows_stay_in_seq_order() {
        let mut events = Vec::new();
        merge_event(&mut events, event("b", 2, "par", false));
        merge_event(&mut events, event("a", 1, "hello", true));
        merge_event(&mut events, event("b", 2, "partial reply", true));
        let texts: Vec<(&str, bool)> = events.iter().map(|e| (e.text.as_str(), e.done)).collect();
        assert_eq!(texts, [("hello", true), ("partial reply", true)]);
    }

    #[test]
    fn following_another_session_starts_empty() {
        let mut live = LiveTranscript::default();
        live.follow(Some(RecordId::new("agent_thread", "t")));
        live.merge(event("a", 5, "hi", true));
        live.loaded = true;
        live.follow(Some(RecordId::new("agent_thread", "t")));
        assert_eq!(live.events().len(), 1, "the same session keeps its rows");
        live.follow(Some(RecordId::new("agent_thread", "u")));
        assert!(live.events().is_empty() && !live.loaded() && live.last_seq == 0);
        live.follow(None);
        assert!(live.thread().is_none());
    }
}

//! Live view of the AI diagnostic a tech just asked for.
//!
//! Without it the tech clicks "Yes, get AI help", the prompt vanishes, and
//! nothing on screen says a run is alive — the first one took seven minutes to
//! open its session. This polls the request, the session it opens, and the
//! entries it writes, so the run is visible while it happens.
//!
//! Everything is looked up by `connection_string` rather than by a threaded
//! record id, so the window still finds the run after a client restart.

use crossbeam::channel::{unbounded, Receiver, Sender};
use displays::ui_tools::{icons, theme};
use eframe::egui::{RichText, ScrollArea, Ui};
use std::time::{Duration, Instant};
use tokio::spawn;

/// Poll cadence. An agent turn runs for minutes, so this is about liveness.
const POLL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub request_status: Option<String>,
    pub request_error: Option<String>,
    pub session_key: Option<String>,
    pub session_status: Option<String>,
    pub diagnosed: bool,
    pub summary: Option<String>,
    /// `(category, title)`, oldest first.
    pub entries: Vec<(String, String)>,
    /// Newest agent message on the conversation, which is what the AI tab shows.
    pub agent_says: Option<String>,
    /// Newest message came from the tech, so the agent has not answered yet.
    pub awaiting_reply: bool,
    pub conversation: usize,
    /// Seconds since the request was handed over, for the stall check.
    pub since_dispatch: Option<i64>,
}

/// How long a handed-over request may go without the agent saying anything or
/// opening a session before the window stops calling it progress. A real turn
/// is quiet for minutes, so this is deliberately past that.
const STALL_SECS: i64 = 420;

pub struct AssistProgress {
    connection_string: String,
    service_number: String,
    snapshot: Snapshot,
    last_poll: Option<Instant>,
    tx: Sender<Snapshot>,
    rx: Receiver<Snapshot>,
}

impl AssistProgress {
    pub fn new(connection_string: String, service_number: String) -> Self {
        let (tx, rx) = unbounded();
        Self {
            connection_string,
            service_number,
            snapshot: Snapshot::default(),
            last_poll: None,
            tx,
            rx,
        }
    }

    /// Drains arrived snapshots and queues the next poll when one is due.
    pub fn tick(&mut self) {
        while let Ok(snap) = self.rx.try_recv() {
            self.snapshot = snap;
        }
        let due = self.last_poll.is_none_or(|t| t.elapsed() >= POLL);
        if !due {
            return;
        }
        self.last_poll = Some(Instant::now());

        let cs = self.connection_string.clone();
        let tx = self.tx.clone();
        spawn(async move {
            if let Some(snap) = fetch(&cs).await {
                let _ = tx.try_send(snap);
            }
        });
    }

    /// One line naming where the run has got to, plus a color for it.
    fn stage(&self) -> (&'static str, StageKind) {
        let s = &self.snapshot;
        if s.diagnosed {
            return ("Root cause identified", StageKind::Done);
        }
        // Quiet past the stall window with no session and nothing said is the
        // shape every silent failure so far has taken: the handover succeeded
        // and the turn then died somewhere the tech cannot see.
        let stalled = s.session_status.is_none()
            && s.agent_says.is_none()
            && s.since_dispatch.is_some_and(|secs| secs > STALL_SECS);

        match (s.session_status.as_deref(), s.request_status.as_deref()) {
            (Some("resolved") | Some("escalated"), _) => ("Diagnostic closed", StageKind::Done),
            (Some(_), _) => ("Agent working on this machine", StageKind::Live),
            (None, Some("failed")) => ("Could not reach the agent", StageKind::Bad),
            _ if stalled => ("No word from the agent — check the AI tab", StageKind::Bad),
            // An answer with no session still means the agent is talking, which
            // the tech can act on; it is not the dead end it used to read as.
            (None, _) if s.agent_says.is_some() => ("Agent replied", StageKind::Done),
            (None, Some("completed")) | (None, Some("dispatched")) => {
                ("Handed to the agent — waiting on its first reply", StageKind::Live)
            },
            (None, Some("pending")) => ("Queued", StageKind::Live),
            _ => ("Requesting…", StageKind::Live),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.tick();
        ui.ctx().request_repaint_after(POLL);

        let (stage, kind) = self.stage();
        let color = match kind {
            StageKind::Done => theme::success(ui),
            StageKind::Live => theme::info(ui),
            StageKind::Bad => theme::error(ui),
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::ROBOT).size(18.0).color(color));
            ui.vertical(|ui| {
                ui.label(RichText::new(stage).strong().size(15.0).color(color));
                ui.label(
                    RichText::new(format!(
                        "service #{}  ·  {}",
                        self.service_number, self.connection_string
                    ))
                    .small()
                    .color(theme::weak_text(ui)),
                );
            });
        });

        // A live turn writes nothing for minutes at a time, so say so rather
        // than leaving an empty pane that reads as broken.
        if matches!(kind, StageKind::Live) {
            ui.add_space(2.);
            ui.label(
                RichText::new("Analysis can take several minutes. This window updates itself.")
                    .small()
                    .color(theme::weak_text(ui)),
            );
        }

        if let Some(err) = self.snapshot.request_error.clone() {
            ui.add_space(4.);
            ui.label(
                RichText::new(err.chars().take(300).collect::<String>())
                    .small()
                    .color(theme::warn(ui)),
            );
        }

        ui.add_space(6.);
        ui.separator();
        ui.add_space(4.);

        // Mirrors the AI tab rather than only the session records, so the two
        // never disagree about what the agent said.
        if let Some(says) = self.snapshot.agent_says.clone() {
            ui.label(
                RichText::new(format!("Agent  ·  {} messages", self.snapshot.conversation))
                    .small()
                    .color(theme::weak_text(ui)),
            );
            ui.label(RichText::new(says.chars().take(1200).collect::<String>()));
            ui.add_space(6.);
        } else if self.snapshot.awaiting_reply {
            ui.label(
                RichText::new("The agent has not answered yet. Reply to it in the AI tab.")
                    .small()
                    .color(theme::weak_text(ui)),
            );
            ui.add_space(6.);
        }

        if let Some(summary) = self.snapshot.summary.clone() {
            ui.label(RichText::new("Summary").small().color(theme::weak_text(ui)));
            ui.label(RichText::new(summary));
            ui.add_space(6.);
        }

        ui.label(
            RichText::new(format!("Findings ({})", self.snapshot.entries.len()))
                .small()
                .color(theme::weak_text(ui)),
        );
        ui.add_space(2.);

        if self.snapshot.entries.is_empty() {
            ui.label(
                RichText::new("Nothing logged yet.")
                    .small()
                    .color(theme::weak_text(ui)),
            );
            return;
        }

        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (category, title) in self.snapshot.entries.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("[{category}]"))
                            .small()
                            .color(category_color(ui, &category)),
                    );
                    ui.label(RichText::new(title).small());
                });
            }
        });
    }
}

enum StageKind {
    Live,
    Done,
    Bad,
}

fn category_color(ui: &Ui, category: &str) -> eframe::egui::Color32 {
    match category {
        "error" | "security_alert" => theme::error(ui),
        "finding" | "recommendation" => theme::info(ui),
        "action" => theme::success(ui),
        _ => theme::weak_text(ui),
    }
}

/// Newest request for this machine, the session it opened, and that session's
/// entries. Three chained queries in one round trip; a missing piece degrades
/// to `None` rather than failing the poll.
async fn fetch(connection_string: &str) -> Option<Snapshot> {
    use database::schema::RecordIdExt;

    let mut res = database::db()
        .query(
            "SELECT id, status, dispatch_error, created_at, \
             math::floor(time::unix(time::now()) - time::unix(dispatched_at ?? created_at)) \
             AS since_dispatch FROM assist_request \
             WHERE connection_string = $cs ORDER BY created_at DESC LIMIT 1",
        )
        .query(
            "SELECT id, status, summary, diagnosed_at, started_at FROM diagnostic_session \
             WHERE connection_string = $cs ORDER BY started_at DESC LIMIT 1",
        )
        .bind(("cs", connection_string.to_string()))
        .await
        .ok()?;

    let requests: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
    let sessions: Vec<serde_json::Value> = res.take(1).unwrap_or_default();

    let str_at = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };

    let request = requests.first();
    let session = sessions.first();

    let mut snap = Snapshot {
        request_status: request.and_then(|r| str_at(r, "status")),
        request_error: request.and_then(|r| str_at(r, "dispatch_error")),
        session_status: session.and_then(|s| str_at(s, "status")),
        summary: session.and_then(|s| str_at(s, "summary")),
        diagnosed: session
            .and_then(|s| s.get("diagnosed_at"))
            .is_some_and(|v| !v.is_null()),
        since_dispatch: request
            .and_then(|r| r.get("since_dispatch"))
            .and_then(serde_json::Value::as_i64),
        ..Default::default()
    };

    // The conversation is what the AI tab shows. Reading it here is what stops
    // this window disagreeing with the chat the tech can already see, and it is
    // the only place an agent's own words reach them when no session opens.
    if let Some(request_id) = request.and_then(|r| str_at(r, "id")) {
        let thread = database::schema::entity_link::parse_record_id(
            &request_id,
            database::schema::ASSIST_REQUEST_TABLE,
        )
        .key_string();
        if let Ok(rows) = database::schema::AssistMessage::thread_history(&thread, 200).await {
            snap.conversation = rows.len();
            snap.awaiting_reply = rows.last().is_some_and(|m| m.direction == "in");
            snap.agent_says = rows
                .iter()
                .rev()
                .find(|m| m.direction == "out")
                .map(|m| m.text.clone());
        }
    }

    let Some(session_id) = session.and_then(|s| str_at(s, "id")) else {
        return Some(snap);
    };
    let session_ref = database::schema::entity_link::parse_record_id(
        &session_id,
        database::schema::DIAGNOSTIC_SESSION_TABLE,
    );
    snap.session_key = Some(session_ref.key_string());

    if let Ok(mut res) = database::db()
        .query(
            "SELECT category, title, timestamp FROM diagnostic_entry \
             WHERE session_ref = $sid ORDER BY timestamp LIMIT 200",
        )
        .bind(("sid", session_ref))
        .await
    {
        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        snap.entries = rows
            .iter()
            .filter_map(|r| {
                Some((
                    str_at(r, "category").unwrap_or_else(|| "note".into()),
                    str_at(r, "title")?,
                ))
            })
            .collect();
    }
    Some(snap)
}

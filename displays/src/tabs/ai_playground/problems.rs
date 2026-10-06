//! Failed AI requests and agent errors across every technician, listed for Root in the Ai tab.

use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{
    AgentProblem, Datetime, ProblemKind, RecordId, RecordIdExt, User, UserAuthorization,
};
use eframe::egui::{self, Color32, Grid, Id, RichText, ScrollArea, Ui};
use web_time::Instant;

use super::session_list::Roster;
use crate::ui_tools::list_row::{Lead, ListRow};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};

const SHOWN_POLL: Duration = Duration::from_secs(20);
const HIDDEN_POLL: Duration = Duration::from_secs(60);
/// A list drawn within this long counts as still open.
const OPEN_GAP: Duration = Duration::from_secs(2);
/// How far back problems count as new before the list was ever opened.
const FIRST_LOOK_SECS: i64 = 24 * 60 * 60;
const HOVER_WIDTH: f32 = 360.0;
const DENIED: &str = "Only a Root user can list problems.";

/// How far back the list reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    Day,
    #[default]
    Week,
    Month,
}

impl Range {
    const ALL: [Self; 3] = [Self::Day, Self::Week, Self::Month];

    fn label(self) -> &'static str {
        match self {
            Self::Day => "24 h",
            Self::Week => "7 days",
            Self::Month => "30 days",
        }
    }

    /// The range as a SurrealDB duration.
    fn window(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "7d",
            Self::Month => "30d",
        }
    }
}

/// A problem clicked in the list.
#[derive(Debug, Clone, PartialEq)]
pub enum ProblemPick {
    /// The session the problem happened in.
    Session { thread: RecordId, is_open: bool },
    /// A problem with no session, shown in the chat area.
    Detail(String),
}

type Read = (Range, Result<Vec<AgentProblem>, String>);

pub struct ProblemsView {
    rows: Vec<AgentProblem>,
    range: Range,
    /// The range the rows were read for.
    loaded: Option<Range>,
    /// The picked problem's key; only problems without a session are picked.
    picked: Option<String>,
    error: Option<String>,
    loading: bool,
    last_poll: Option<Instant>,
    /// Unix seconds up to which the list has been seen, kept across restarts.
    seen_until: Option<i64>,
    /// `seen_until` as it stood when the list was opened; rows past it draw as new.
    seen_at_open: Option<i64>,
    last_drawn: Option<Instant>,
    tx: Sender<Read>,
    rx: Receiver<Read>,
}

impl Default for ProblemsView {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            rows: Vec::new(),
            range: Range::default(),
            loaded: None,
            picked: None,
            error: None,
            loading: false,
            last_poll: None,
            seen_until: None,
            seen_at_open: None,
            last_drawn: None,
            tx,
            rx,
        }
    }
}

fn seen_id() -> Id {
    Id::new("enhanced_ai_problems_seen_until")
}

fn unix(at: &Datetime) -> i64 {
    DateTime::<Utc>::from(*at).timestamp()
}

fn local(at: &Datetime) -> String {
    DateTime::<Utc>::from(*at)
        .with_timezone(&Local)
        .format("%b %d %H:%M")
        .to_string()
}

/// Icon and colour for a problem: red for a failure, amber for a request lost or still waiting.
fn mark(ui: &Ui, kind: ProblemKind) -> (&'static str, Color32) {
    match kind {
        ProblemKind::Waiting | ProblemKind::Stuck => (icons::STATUS_WAIT, theme::warn(ui)),
        ProblemKind::NotPickedUp => (icons::STATUS_WARN, theme::warn(ui)),
        _ => (icons::STATUS_ERR, theme::error(ui)),
    }
}

/// The requester's name, else `Unattributed`.
fn tech(problem: &AgentProblem, roster: &Roster) -> String {
    problem
        .requested_by
        .as_deref()
        .filter(|e| !e.trim().is_empty())
        .map_or_else(
            || "Unattributed".to_string(),
            |email| roster.name_for(email),
        )
}

/// Whether the lowercased `needle` is in the problem's technician, store, subject, headline or error.
fn mentions(problem: &AgentProblem, tech: &str, needle: &str) -> bool {
    needle.is_empty()
        || [
            tech,
            problem.requested_by.as_deref().unwrap_or(""),
            problem.store.as_deref().unwrap_or(""),
            &problem.subject(),
            &problem.headline(),
            &problem.message,
        ]
        .iter()
        .any(|f| f.to_lowercase().contains(needle))
}

/// What a technician or Root should take from a problem that never got a session.
fn explanation(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::NotPickedUp => {
            "No agent session was opened. When several requests in a row end this way, admin-agent was down or not claiming requests."
        }
        ProblemKind::Waiting => {
            "No agent session has opened yet. If this stays, check that admin-agent is running."
        }
        ProblemKind::RequestFailed => {
            "The broker refused the request or could not open a session for it."
        }
        _ => "",
    }
}

impl ProblemsView {
    /// Applies finished reads and starts due ones, faster while the list is `shown`.
    pub fn tick(&mut self, ui: &Ui, shown: bool) {
        self.drain();
        if self.seen_until.is_none() {
            self.seen_until = ui.data_mut(|d| d.get_persisted::<i64>(seen_id()));
        }
        let every = if shown { SHOWN_POLL } else { HIDDEN_POLL };
        let stale = self.loaded != Some(self.range);
        if !self.loading && (stale || self.last_poll.is_none_or(|t| t.elapsed() >= every)) {
            self.loading = true;
            self.last_poll = Some(Instant::now());
            let tx = self.tx.clone();
            let range = self.range;
            let ctx = ui.ctx().clone();
            PlatformSpawner::spawn(async move {
                // Root is checked here, not taken from the caller's cached flag.
                let me = User::get_current_user_from_auth().await.ok().flatten();
                let root = me.is_some_and(|u| u.get_authorization() == UserAuthorization::Root);
                let read = if root {
                    AgentProblem::list(range.window())
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    Err(DENIED.to_string())
                };
                let _ = tx.send((range, read));
                ctx.request_repaint();
            });
        }
        ui.ctx().request_repaint_after(every);
    }

    /// Problems newer than the last look at the list, for the Problems button.
    pub fn unseen_count(&self) -> usize {
        let since = self
            .seen_until
            .unwrap_or_else(|| Utc::now().timestamp() - FIRST_LOOK_SECS);
        self.rows
            .iter()
            .filter(|p| p.at.as_ref().is_some_and(|at| unix(at) > since))
            .count()
    }

    pub fn picked(&self) -> Option<String> {
        self.picked.clone()
    }

    pub fn select(&mut self, key: &str) {
        self.picked = Some(key.to_string());
    }

    pub fn deselect(&mut self) {
        self.picked = None;
    }

    /// The picked problem's subject, for the top bar.
    pub fn title(&self) -> Option<String> {
        let key = self.picked.as_ref()?;
        let row = self.rows.iter().find(|p| &p.key == key);
        Some(row.map_or_else(
            || "Problem".to_string(),
            |p| format!("Problem \u{00b7} {}", p.subject()),
        ))
    }

    /// The range buttons and the reading spinner.
    pub fn range_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for range in Range::ALL {
                ui.selectable_value(&mut self.range, range, range.label());
            }
            if self.loading {
                ui.spinner();
            }
        });
    }

    /// The problems matching `needle`, newest first; `selected_thread` marks the rows of the open session. Returns the row clicked.
    pub fn list_ui(
        &mut self,
        ui: &mut Ui,
        needle: &str,
        roster: &Roster,
        selected_thread: &str,
    ) -> Option<ProblemPick> {
        crate::ui_data::agent_problem_notify::mark_list_in_view();
        self.mark_seen(ui);
        if let Some(err) = &self.error {
            ui.label(
                RichText::new(format!("{} {err}", icons::STATUS_ERR))
                    .color(theme::error(ui))
                    .small(),
            );
        }
        let needle = needle.trim().to_lowercase();
        let rows: Vec<(&AgentProblem, String)> = self
            .rows
            .iter()
            .map(|p| (p, tech(p, roster)))
            .filter(|(p, tech)| mentions(p, tech, &needle))
            .collect();
        if rows.is_empty() {
            let empty = match (self.loaded.is_none(), needle.is_empty()) {
                (true, _) => "Loading\u{2026}",
                (false, true) => "No problems in this range",
                (false, false) => "No matching problems",
            };
            ui.label(RichText::new(empty).color(theme::weak_text(ui)).small());
        }
        let new_after = self.seen_at_open.unwrap_or(i64::MIN);
        let mut clicked = None;
        for (problem, tech) in rows {
            let (icon, color) = mark(ui, problem.kind);
            let title = format!("{tech} \u{00b7} {}", problem.subject());
            let mut detail = problem.headline();
            if let Some(store) = problem.store.as_deref().filter(|s| !s.is_empty()) {
                detail.push_str(&format!(" \u{00b7} {store}"));
            }
            if let Some(at) = &problem.at {
                detail.push_str(&format!(" \u{00b7} {}", local(at)));
            }
            let selected = match &problem.thread {
                Some(thread) => thread.key_string() == selected_thread,
                None => self.picked.as_deref() == Some(problem.key.as_str()),
            };
            let unread = problem.at.as_ref().is_some_and(|at| unix(at) > new_after);
            let resp = ListRow::new(&title)
                .lead(Lead::Icon(icon, Some(color)))
                .detail(&detail)
                .selected(selected)
                .unread(unread)
                .show(ui)
                .on_hover_ui(|ui| {
                    ui.set_max_width(HOVER_WIDTH);
                    ui.label(RichText::new(problem.kind.label()).strong().color(color));
                    ui.label(&title);
                    ui.label(RichText::new(&problem.message).small().monospace());
                });
            if resp.clicked() {
                clicked = Some(match &problem.thread {
                    Some(thread) => ProblemPick::Session {
                        thread: thread.clone(),
                        is_open: problem.thread_is_open(),
                    },
                    None => ProblemPick::Detail(problem.key.clone()),
                });
            }
        }
        clicked
    }

    /// The picked problem, which has no session to open.
    pub fn detail_ui(&mut self, ui: &mut Ui, roster: &Roster) {
        let Some(key) = self.picked.clone() else {
            return;
        };
        let Some(problem) = self.rows.iter().find(|p| p.key == key) else {
            let text = if self.loaded.is_none() {
                "Loading\u{2026}"
            } else {
                "This problem is no longer listed; a waiting request may have been picked up since."
            };
            ui.label(RichText::new(text).color(theme::weak_text(ui)));
            return;
        };
        let (icon, color) = mark(ui, problem.kind);
        ui.label(
            RichText::new(format!("{icon} {}", problem.headline()))
                .heading()
                .color(color),
        );
        ui.label(
            RichText::new(problem.kind.label())
                .color(theme::weak_text(ui))
                .small(),
        );
        ui.add_space(6.0);
        let machine = problem
            .hostname
            .clone()
            .unwrap_or_else(|| problem.connection_string.clone());
        let fields = [
            ("Technician", tech(problem, roster)),
            ("Store", problem.store.clone().unwrap_or_default()),
            ("Machine", machine),
            (
                "Service order",
                problem.service_number.clone().unwrap_or_default(),
            ),
            ("When", problem.at.as_ref().map(local).unwrap_or_default()),
        ];
        Grid::new("enhanced_ai_problem_detail")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (name, value) in fields.iter().filter(|(_, v)| !v.is_empty()) {
                    ui.label(RichText::new(*name).color(theme::weak_text(ui)));
                    ui.label(value);
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        let explained = explanation(problem.kind);
        if !explained.is_empty() {
            ui.label(explained);
        }
        if !problem.message.trim().is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Recorded error")
                    .color(theme::weak_text(ui))
                    .small(),
            );
            ScrollArea::vertical()
                .id_salt("enhanced_ai_problem_message")
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&problem.message).monospace())
                            .selectable(true),
                    );
                });
        }
    }

    /// Moves the seen mark to now while the list is drawn, keeping the mark from when it opened for the new-row highlight.
    fn mark_seen(&mut self, ui: &Ui) {
        let reopened = self.last_drawn.is_none_or(|t| t.elapsed() > OPEN_GAP);
        if reopened {
            self.seen_at_open = Some(
                self.seen_until
                    .unwrap_or_else(|| Utc::now().timestamp() - FIRST_LOOK_SECS),
            );
        }
        self.last_drawn = Some(Instant::now());
        let now = Utc::now().timestamp();
        if self.seen_until != Some(now) {
            self.seen_until = Some(now);
            ui.data_mut(|d| d.insert_persisted(seen_id(), now));
        }
    }

    fn drain(&mut self) {
        while let Ok((range, read)) = self.rx.try_recv() {
            self.loading = false;
            if range != self.range {
                continue;
            }
            match read {
                Ok(rows) => {
                    self.rows = rows;
                    self.loaded = Some(range);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    #[cfg(test)]
    pub(super) fn set_rows(&mut self, rows: Vec<AgentProblem>) {
        self.rows = rows;
        self.loaded = Some(self.range);
        self.last_poll = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(key: &str, thread: Option<&str>, at_secs: Option<i64>) -> AgentProblem {
        AgentProblem {
            key: key.to_string(),
            kind: if thread.is_some() {
                ProblemKind::AgentError
            } else {
                ProblemKind::NotPickedUp
            },
            at: at_secs.and_then(|s| Datetime::from_timestamp(s, 0)),
            requested_by: Some("joshua.adams@pclaptops.com".to_string()),
            store: Some("RIV".to_string()),
            hostname: Some("JeffsComputer".to_string()),
            connection_string: "JeffsComputer:663a3fd40".to_string(),
            service_number: Some("2155485".to_string()),
            title: None,
            thread: thread.map(|t| RecordId::new("agent_thread", t)),
            thread_status: thread.map(|_| "idle".to_string()),
            request: None,
            message: "Agent error: exceeded retry limit, last status: 429 Too Many Requests"
                .to_string(),
        }
    }

    #[test]
    fn the_search_matches_technician_store_machine_and_error() {
        let p = problem("a", Some("t1"), None);
        for needle in [
            "joshua",
            "riv",
            "jeffscomputer",
            "2155485",
            "busy",
            "429",
            "",
        ] {
            assert!(mentions(&p, "Joshua Adams", needle), "{needle}");
        }
        assert!(!mentions(&p, "Joshua Adams", "green-machine"));
    }

    #[test]
    fn unseen_counts_rows_after_the_seen_mark() {
        let mut view = ProblemsView::default();
        let now = Utc::now().timestamp();
        view.set_rows(vec![
            problem("old", None, Some(now - 600)),
            problem("new", None, Some(now - 60)),
            problem("untimed", None, None),
        ]);
        view.seen_until = Some(now - 300);
        assert_eq!(view.unseen_count(), 1);
        view.seen_until = None;
        assert_eq!(
            view.unseen_count(),
            2,
            "before the first look, the last day counts as new"
        );
    }

    #[test]
    fn a_read_for_an_earlier_range_is_dropped() {
        let mut view = ProblemsView::default();
        view.tx
            .send((Range::Day, Ok(vec![problem("a", None, None)])))
            .expect("send");
        view.drain();
        assert!(view.rows.is_empty());
        assert_eq!(view.loaded, None);
        view.tx
            .send((Range::Week, Ok(vec![problem("b", None, None)])))
            .expect("send");
        view.drain();
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.loaded, Some(Range::Week));
    }

    #[test]
    fn the_title_names_the_picked_problem() {
        let mut view = ProblemsView::default();
        view.set_rows(vec![problem("r1", None, None)]);
        assert_eq!(view.title(), None);
        view.select("r1");
        assert_eq!(
            view.title().as_deref(),
            Some("Problem \u{00b7} #2155485 JeffsComputer")
        );
        view.select("gone");
        assert_eq!(view.title().as_deref(), Some("Problem"));
        view.deselect();
        assert_eq!(view.picked(), None);
    }
}

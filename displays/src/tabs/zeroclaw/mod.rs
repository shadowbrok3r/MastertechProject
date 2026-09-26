//! ZeroClaw gateway sessions and automations for the Ai tab's session list (Root only): each
//! agent's sessions and transcripts, and the scheduled automations with their runs, read from the
//! same API the zc-codex app uses.

mod api;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local};
use crossbeam::channel::{Receiver, Sender};
use database::schema::ZeroclawGateway;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Align, Color32, Id, Layout, RichText, ScrollArea, TextStyle, Ui};
use web_time::Instant;

use crate::ui_tools::chat_bubble::{self, ChatKind, ChatRow, ChatStyle};
use crate::ui_tools::list_row::{Lead, ListRow};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use api::{Automation, Item, Outcome, Run, SessionRow};

/// Session and automation list poll while the list is drawn.
const LIST_POLL: Duration = Duration::from_secs(30);
/// Transcript poll while its session has a turn running, and otherwise.
const TRANSCRIPT_POLL_BUSY: Duration = Duration::from_secs(5);
const TRANSCRIPT_POLL_IDLE: Duration = Duration::from_secs(60);
const RUNS_POLL: Duration = Duration::from_secs(30);
const RUNS_SHOWN: usize = 20;
const SUMMARY_CHARS: usize = 160;
const DENIED: &str = "Only an active Root user can browse ZeroClaw.";
/// Frames a newly opened transcript is scrolled to its end, while its rows settle their heights.
const SCROLL_TO_END_FRAMES: u8 = 3;

/// Message counts the viewer has read, by session id; kept across restarts.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Seen {
    /// Set once the first session list has been recorded as read.
    baselined: bool,
    sessions: HashMap<String, u64>,
}

fn seen_id() -> Id {
    Id::new("zeroclaw_seen")
}

/// A ZeroClaw item picked from the session list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZeroClawPick {
    Session(String),
    Automation(String),
}

enum Msg {
    Gateway(Result<Option<ZeroclawGateway>, String>),
    Sessions(Result<(Vec<SessionRow>, Vec<String>), String>),
    Transcript(String, Result<Vec<Item>, String>),
    Jobs(Result<Vec<Automation>, String>),
    Runs(String, Result<Vec<Run>, String>),
    RanNow(String, Result<(Outcome, String), String>),
}

enum Access {
    Unknown,
    Loading,
    Ready(ZeroclawGateway),
    Denied,
    Failed(String),
}

pub struct ZeroClawView {
    access: Access,
    sessions: Vec<SessionRow>,
    running: HashSet<String>,
    /// The picked session; polled while set.
    session: Option<String>,
    transcript: Vec<Item>,
    transcript_for: Option<String>,
    jobs: Vec<Automation>,
    /// The picked automation; its runs are polled while set.
    job: Option<String>,
    runs: Vec<Run>,
    runs_for: Option<String>,
    /// Automations with a run-now in flight.
    starting: HashSet<String>,
    error: Option<String>,
    notice: Option<String>,
    loading_lists: bool,
    loading_transcript: bool,
    loading_runs: bool,
    last_lists: Option<Instant>,
    last_transcript: Option<Instant>,
    last_runs: Option<Instant>,
    /// Loaded from egui memory on the first frame.
    seen: Option<Seen>,
    seen_dirty: bool,
    scroll_to_end: u8,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Default for ZeroClawView {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            access: Access::Unknown,
            sessions: Vec::new(),
            running: HashSet::new(),
            session: None,
            transcript: Vec::new(),
            transcript_for: None,
            jobs: Vec::new(),
            job: None,
            runs: Vec::new(),
            runs_for: None,
            starting: HashSet::new(),
            error: None,
            notice: None,
            loading_lists: false,
            loading_transcript: false,
            loading_runs: false,
            last_lists: None,
            last_transcript: None,
            last_runs: None,
            seen: None,
            seen_dirty: false,
            scroll_to_end: 0,
            tx,
            rx,
        }
    }
}

fn due(last: Option<Instant>, every: Duration) -> bool {
    last.is_none_or(|t| t.elapsed() >= every)
}

/// An RFC 3339 stamp in local time, or the stamp itself when it does not parse.
fn local(ts: &str) -> String {
    DateTime::parse_from_rfc3339(ts)
        .map(|d| d.with_timezone(&Local).format("%b %d %H:%M").to_string())
        .unwrap_or_else(|_| ts.chars().take(16).collect::<String>().replacen('T', " ", 1))
}

fn stamp(ts: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(ts).ok()
}

/// Sessions newest first; unparsable stamps last.
fn newest_first(rows: &mut [SessionRow]) {
    rows.sort_by_key(|r| std::cmp::Reverse(stamp(&r.last_activity)));
}

/// Rows grouped by agent, in the order of each agent's first row.
fn group_by_agent(rows: &[SessionRow]) -> Vec<(&str, Vec<&SessionRow>)> {
    let mut groups: Vec<(&str, Vec<&SessionRow>)> = Vec::new();
    for row in rows {
        match groups.iter_mut().find(|(agent, _)| *agent == row.agent) {
            Some((_, group)) => group.push(row),
            None => groups.push((row.agent.as_str(), vec![row])),
        }
    }
    groups
}

/// A group's header: its name, row count and new-message count.
fn group_header(ui: &Ui, name: &str, count: usize, unread: usize) -> LayoutJob {
    let font = TextStyle::Body.resolve(ui.style());
    let mut job = LayoutJob::default();
    let name = if name.is_empty() { "no agent" } else { name };
    job.append(name, 0.0, TextFormat::simple(font.clone(), ui.visuals().text_color()));
    job.append(&format!("  {count}"), 0.0, TextFormat::simple(font.clone(), theme::weak_text(ui)));
    if unread > 0 {
        job.append(&format!("  \u{00b7} {unread} new"), 0.0, TextFormat::simple(font, theme::accent(ui)));
    }
    job
}

/// Whether any field contains the lowercased `needle`; an empty needle matches everything.
fn mentions(needle: &str, fields: &[&str]) -> bool {
    needle.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(needle))
}

impl ZeroClawView {
    /// Loads read state, applies finished requests, starts due polls and marks the shown session read.
    pub fn tick(&mut self, ui: &Ui) {
        if self.seen.is_none() {
            self.seen = Some(ui.data_mut(|d| d.get_persisted::<Seen>(seen_id())).unwrap_or_default());
        }
        self.drain();
        match self.access {
            Access::Unknown => self.load_gateway(),
            Access::Ready(_) => self.poll(),
            Access::Loading | Access::Denied | Access::Failed(_) => {}
        }
        if let Some(id) = self.shown_session() {
            self.mark_read(&id);
        }
        if self.seen_dirty
            && let Some(seen) = &self.seen
        {
            ui.data_mut(|d| d.insert_persisted(seen_id(), seen.clone()));
            self.seen_dirty = false;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    /// Sessions with messages the viewer has not read.
    pub fn unread_count(&self) -> usize {
        self.sessions.iter().filter(|s| self.unread(s)).count()
    }

    /// The picked item, if any.
    pub fn picked(&self) -> Option<ZeroClawPick> {
        self.session.clone().map(ZeroClawPick::Session).or_else(|| self.job.clone().map(ZeroClawPick::Automation))
    }

    /// Shows `pick` in the detail view and polls its transcript or runs.
    pub fn select(&mut self, pick: &ZeroClawPick) {
        match pick {
            ZeroClawPick::Session(id) => {
                if self.session.as_deref() != Some(id.as_str()) {
                    self.session = Some(id.clone());
                    self.transcript.clear();
                    self.transcript_for = None;
                }
                self.job = None;
            }
            ZeroClawPick::Automation(id) => {
                if self.job.as_deref() != Some(id.as_str()) {
                    self.job = Some(id.clone());
                    self.runs.clear();
                    self.runs_for = None;
                }
                self.session = None;
            }
        }
        self.error = None;
    }

    /// Stops showing and polling any item.
    pub fn deselect(&mut self) {
        self.session = None;
        self.job = None;
    }

    /// The picked item's name for the top bar.
    pub fn title(&self) -> Option<String> {
        if let Some(id) = &self.session {
            let row = self.sessions.iter().find(|s| &s.id == id);
            return Some(row.map_or_else(|| id.clone(), |s| s.label().to_string()));
        }
        let id = self.job.as_ref()?;
        let job = self.jobs.iter().find(|j| &j.id == id);
        Some(job.map_or_else(|| id.clone(), |j| j.label().to_string()))
    }

    /// Automations, then each agent's sessions newest first, filtered by `needle`; returns the item clicked.
    pub fn list_ui(&mut self, ui: &mut Ui, needle: &str) -> Option<ZeroClawPick> {
        match &self.access {
            Access::Unknown | Access::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Connecting to ZeroClaw\u{2026}").color(theme::weak_text(ui)).small());
                });
                return None;
            }
            Access::Denied => {
                ui.label(RichText::new(DENIED).color(theme::weak_text(ui)).small());
                return None;
            }
            Access::Failed(why) => {
                ui.label(RichText::new(format!("{} {why}", icons::STATUS_ERR)).color(theme::error(ui)).small());
                if ui.button(format!("{} Retry", icons::REFRESH)).clicked() {
                    self.access = Access::Unknown;
                }
                return None;
            }
            Access::Ready(_) => {}
        }
        let needle = needle.trim().to_lowercase();
        let searching = !needle.is_empty();
        let mut picked = None;

        let jobs: Vec<Automation> = self
            .jobs
            .iter()
            .filter(|j| mentions(&needle, &[j.label(), &j.id, &j.agent_alias]))
            .cloned()
            .collect();
        if !jobs.is_empty() {
            let unread = jobs.iter().filter(|j| self.job_unread(j)).count();
            egui::CollapsingHeader::new(group_header(ui, "Automations", jobs.len(), unread))
                .id_salt("zeroclaw_automations")
                .default_open(true)
                .open(searching.then_some(true))
                .show(ui, |ui| {
                    for job in &jobs {
                        if self.job_row(ui, job) {
                            picked = Some(ZeroClawPick::Automation(job.id.clone()));
                        }
                    }
                });
        }

        let mut rows: Vec<SessionRow> = self
            .sessions
            .iter()
            .filter(|s| mentions(&needle, &[s.label(), &s.agent, &s.id]))
            .cloned()
            .collect();
        newest_first(&mut rows);
        for (agent, group) in group_by_agent(&rows) {
            let unread = group.iter().filter(|r| self.unread(r)).count();
            egui::CollapsingHeader::new(group_header(ui, agent, group.len(), unread))
                .id_salt(("zeroclaw_group", agent))
                .default_open(false)
                .open(searching.then_some(true))
                .show(ui, |ui| {
                    for row in group {
                        if self.session_row(ui, row) {
                            picked = Some(ZeroClawPick::Session(row.id.clone()));
                        }
                    }
                });
        }

        if jobs.is_empty() && rows.is_empty() {
            let empty = if searching { "No matches" } else { "No sessions" };
            ui.label(RichText::new(empty).color(theme::weak_text(ui)).small());
        }
        picked
    }

    /// The picked session's transcript or automation's runs.
    pub fn detail_ui(&mut self, ui: &mut Ui) {
        match &self.access {
            Access::Ready(_) => {}
            Access::Denied => {
                ui.label(RichText::new(DENIED).color(theme::weak_text(ui)));
                return;
            }
            Access::Failed(why) => {
                ui.label(RichText::new(format!("{} {why}", icons::STATUS_ERR)).color(theme::error(ui)));
                return;
            }
            Access::Unknown | Access::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting to ZeroClaw\u{2026}");
                });
                return;
            }
        }
        if self.session.is_some() {
            self.transcript_ui(ui);
        } else if self.job.is_some() {
            self.runs_ui(ui);
        }
    }

    /// The session whose messages are on screen: the picked session, or the picked automation's result session.
    fn shown_session(&self) -> Option<String> {
        if let Some(id) = &self.session {
            return Some(id.clone());
        }
        let job = self.job.as_ref()?;
        self.jobs.iter().find(|j| &j.id == job).map(Automation::session_id)
    }

    /// Whether `row` has messages the viewer has not read.
    fn unread(&self, row: &SessionRow) -> bool {
        self.seen
            .as_ref()
            .is_some_and(|s| s.baselined && row.messages > s.sessions.get(&row.id).copied().unwrap_or(0))
    }

    /// Whether the session `job` delivers its results into has unread messages.
    fn job_unread(&self, job: &Automation) -> bool {
        let session = job.session_id();
        self.sessions.iter().find(|s| s.id == session).is_some_and(|s| self.unread(s))
    }

    /// Records the session `id` as read up to its current message count.
    fn mark_read(&mut self, id: &str) {
        let Some(count) = self.sessions.iter().find(|s| s.id == id).map(|s| s.messages) else {
            return;
        };
        let seen = self.seen.get_or_insert_with(Seen::default);
        if seen.sessions.get(id) != Some(&count) {
            seen.sessions.insert(id.to_string(), count);
            self.seen_dirty = true;
        }
    }

    /// Records every listed session as read the first time a list arrives.
    fn baseline(&mut self) {
        let seen = self.seen.get_or_insert_with(Seen::default);
        if seen.baselined {
            return;
        }
        for row in &self.sessions {
            seen.sessions.insert(row.id.clone(), row.messages);
        }
        seen.baselined = true;
        self.seen_dirty = true;
    }

    fn gateway(&self) -> Option<ZeroclawGateway> {
        match &self.access {
            Access::Ready(gw) => Some(gw.clone()),
            _ => None,
        }
    }

    fn load_gateway(&mut self) {
        self.access = Access::Loading;
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let _ = tx.send(Msg::Gateway(ZeroclawGateway::fetch().await.map_err(|e| e.to_string())));
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Gateway(Ok(Some(gw))) => self.access = Access::Ready(gw),
                Msg::Gateway(Ok(None)) => self.access = Access::Denied,
                Msg::Gateway(Err(e)) => self.access = Access::Failed(e),
                Msg::Sessions(r) => {
                    self.loading_lists = false;
                    match r {
                        Ok((sessions, running)) => {
                            self.sessions = sessions;
                            self.running = running.into_iter().collect();
                            self.error = None;
                            self.baseline();
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Msg::Jobs(Ok(jobs)) => self.jobs = jobs,
                Msg::Jobs(Err(e)) => self.error = Some(e),
                Msg::Transcript(id, r) => {
                    self.loading_transcript = false;
                    if self.session.as_deref() == Some(id.as_str()) {
                        match r {
                            Ok(items) => {
                                if self.transcript_for.as_deref() != Some(id.as_str()) {
                                    self.scroll_to_end = SCROLL_TO_END_FRAMES;
                                }
                                self.transcript = items;
                                self.transcript_for = Some(id);
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
                Msg::Runs(job, r) => {
                    self.loading_runs = false;
                    if self.job.as_deref() == Some(job.as_str()) {
                        match r {
                            Ok(runs) => {
                                self.runs = runs;
                                self.runs_for = Some(job);
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
                Msg::RanNow(job, r) => {
                    self.starting.remove(&job);
                    self.notice = Some(match r {
                        Ok((outcome, _)) => format!("{job}: {}", outcome.label()),
                        Err(e) => format!("{job}: {e}"),
                    });
                    self.last_lists = None;
                    self.last_runs = None;
                }
            }
        }
    }

    fn poll(&mut self) {
        let Some(gw) = self.gateway() else { return };
        if !self.loading_lists && due(self.last_lists, LIST_POLL) {
            self.loading_lists = true;
            self.last_lists = Some(Instant::now());
            let tx = self.tx.clone();
            let gw = gw.clone();
            PlatformSpawner::spawn(async move {
                let _ = tx.send(Msg::Jobs(api::automations(&gw).await.map_err(|e| e.to_string())));
                let lists = async { Ok::<_, anyhow::Error>((api::sessions(&gw).await?, api::running(&gw).await.unwrap_or_default())) };
                let _ = tx.send(Msg::Sessions(lists.await.map_err(|e| e.to_string())));
            });
        }
        if let Some(id) = self.session.clone() {
            let every = if self.running.contains(&id) { TRANSCRIPT_POLL_BUSY } else { TRANSCRIPT_POLL_IDLE };
            let stale = self.transcript_for.as_deref() != Some(id.as_str());
            if !self.loading_transcript
                && (stale || due(self.last_transcript, every))
                && let Some(row) = self.sessions.iter().find(|s| s.id == id).cloned()
            {
                self.loading_transcript = true;
                self.last_transcript = Some(Instant::now());
                let tx = self.tx.clone();
                let gw = gw.clone();
                PlatformSpawner::spawn(async move {
                    let _ = tx.send(Msg::Transcript(row.id.clone(), api::transcript(&gw, &row).await.map_err(|e| e.to_string())));
                });
            }
        }
        if let Some(job) = self.job.clone() {
            let stale = self.runs_for.as_deref() != Some(job.as_str());
            if !self.loading_runs && (stale || due(self.last_runs, RUNS_POLL)) {
                self.loading_runs = true;
                self.last_runs = Some(Instant::now());
                let tx = self.tx.clone();
                PlatformSpawner::spawn(async move {
                    let _ = tx.send(Msg::Runs(job.clone(), api::runs(&gw, &job, RUNS_SHOWN).await.map_err(|e| e.to_string())));
                });
            }
        }
    }

    /// One session row; returns whether an unselected row was clicked.
    fn session_row(&self, ui: &mut Ui, row: &SessionRow) -> bool {
        let selected = self.session.as_deref() == Some(row.id.as_str());
        let icon = if row.is_automation() { icons::AUTOMATION } else { icons::CHAT };
        let lead = if self.running.contains(&row.id) { Lead::Spinner(None) } else { Lead::Icon(icon, None) };
        let detail = format!("{} msg \u{00b7} {}", row.messages, local(&row.last_activity));
        let resp = ListRow::new(row.label())
            .lead(lead)
            .detail(&detail)
            .selected(selected)
            .unread(self.unread(row))
            .show(ui)
            .on_hover_text(format!("{}\n{} \u{00b7} {}", row.label(), row.agent, row.id));
        resp.clicked() && !selected
    }

    /// One automation row; returns whether an unselected row was clicked.
    fn job_row(&self, ui: &mut Ui, job: &Automation) -> bool {
        let selected = self.job.as_deref() == Some(job.id.as_str());
        let (icon, color) = outcome_mark(ui, job.outcome());
        let lead = if self.starting.contains(&job.id) { Lead::Spinner(None) } else { Lead::Icon(icon, Some(color)) };
        let last = job.last_run.as_deref().map(local).unwrap_or_else(|| "never".to_string());
        let next = job.next_run.as_deref().map(local).unwrap_or_else(|| "\u{2014}".to_string());
        let state = if job.enabled { "" } else { " \u{00b7} paused" };
        let detail = format!("{} \u{00b7} last {last} \u{00b7} next {next}{state}", job.expression);
        let resp = ListRow::new(job.label())
            .lead(lead)
            .detail(&detail)
            .selected(selected)
            .unread(self.job_unread(job))
            .show(ui)
            .on_hover_text(format!("{}\n{} \u{00b7} {}\n{detail}", job.label(), job.agent_alias, job.id));
        resp.clicked() && !selected
    }

    /// Refresh, a spinner while loading, and the last error or notice, laid out right to left.
    fn status_tail(&mut self, ui: &mut Ui) {
        if ui.button(icons::REFRESH).on_hover_text("Refresh now").clicked() {
            self.last_lists = None;
            self.last_transcript = None;
            self.last_runs = None;
        }
        if self.loading_lists || self.loading_transcript || self.loading_runs {
            ui.spinner();
        }
        if let Some(e) = &self.error {
            ui.add(egui::Label::new(RichText::new(format!("{} {e}", icons::STATUS_ERR)).color(theme::error(ui)).small()).truncate());
        } else if let Some(n) = &self.notice {
            ui.add(egui::Label::new(RichText::new(n).color(theme::weak_text(ui)).small()).truncate());
        }
    }

    fn transcript_ui(&mut self, ui: &mut Ui) {
        let Some(row) = self.session.as_ref().and_then(|id| self.sessions.iter().find(|s| &s.id == id)).cloned() else {
            ui.label(RichText::new("This session is no longer listed.").color(theme::weak_text(ui)));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(row.label()).strong());
            ui.label(RichText::new(format!("\u{00b7} {} \u{00b7} {}", row.agent, row.id)).color(theme::weak_text(ui)));
            if self.running.contains(&row.id) {
                ui.label(RichText::new("\u{00b7} working").color(theme::accent(ui)));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.status_tail(ui));
        });
        ui.separator();
        if self.transcript_for.as_deref() != Some(row.id.as_str()) {
            ui.spinner();
            return;
        }
        let style = ChatStyle::from_ui(ui);
        let scope = Id::new(("zeroclaw_transcript", row.id.as_str()));
        let agent = row.agent.clone();
        let to_end = self.scroll_to_end > 0;
        ScrollArea::vertical()
            .id_salt(("zeroclaw_transcript_scroll", row.id.as_str()))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (i, item) in self.transcript.iter().enumerate() {
                    item_row(ui, &style, scope, &format!("{i}"), &agent, item);
                }
                if to_end {
                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                }
            });
        if to_end {
            self.scroll_to_end -= 1;
            ui.ctx().request_repaint();
        }
    }

    fn runs_ui(&mut self, ui: &mut Ui) {
        let Some(job) = self.job.as_ref().and_then(|id| self.jobs.iter().find(|j| &j.id == id)).cloned() else {
            ui.label(RichText::new("This automation is no longer listed.").color(theme::weak_text(ui)));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(job.label()).strong());
            ui.label(RichText::new(format!("\u{00b7} {} \u{00b7} {}", job.agent_alias, job.expression)).color(theme::weak_text(ui)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.starting.contains(&job.id);
                let run = ui
                    .add_enabled(!busy, egui::Button::new(format!("{} Run now", icons::PLAY)))
                    .on_hover_text("Run this automation now; its result also goes to its session");
                if run.clicked()
                    && let Some(gw) = self.gateway()
                {
                    self.starting.insert(job.id.clone());
                    self.notice = Some(format!("{}: running\u{2026}", job.id));
                    let tx = self.tx.clone();
                    let id = job.id.clone();
                    PlatformSpawner::spawn(async move {
                        let _ = tx.send(Msg::RanNow(id.clone(), api::run_now(&gw, &id).await.map_err(|e| e.to_string())));
                    });
                }
                if busy {
                    ui.spinner();
                }
                self.status_tail(ui);
            });
        });
        ui.separator();
        if self.runs_for.as_deref() != Some(job.id.as_str()) {
            ui.spinner();
            return;
        }
        if self.runs.is_empty() {
            ui.label(RichText::new("No runs yet").color(theme::weak_text(ui)));
            return;
        }
        let style = ChatStyle::from_ui(ui);
        let scope = Id::new(("zeroclaw_runs", job.id.as_str()));
        ScrollArea::vertical().id_salt(("zeroclaw_runs_scroll", job.id.as_str())).auto_shrink([false, false]).show(ui, |ui| {
            for (i, run) in self.runs.iter().enumerate() {
                run_row(ui, &style, scope, i, run);
            }
        });
    }
}

fn outcome_mark(ui: &Ui, outcome: Outcome) -> (&'static str, Color32) {
    match outcome {
        Outcome::Ok => (icons::STATUS_ON, theme::success(ui)),
        Outcome::Degraded => (icons::STATUS_WARN, theme::warn(ui)),
        Outcome::Failed => (icons::STATUS_ERR, theme::error(ui)),
        Outcome::Skipped | Outcome::Unknown => (icons::STATUS_OFF, theme::weak_text(ui)),
    }
}

fn item_row(ui: &mut Ui, style: &ChatStyle, scope: Id, key: &str, agent: &str, item: &Item) {
    match item {
        Item::User { text, at } => {
            ChatRow::new(ChatKind::User, key, "User")
                .time(Some(local(at)))
                .copy(text)
                .show(ui, style, scope, |ui, id| chat_bubble::markdown(ui, style, text, style.text, id));
        }
        Item::Agent { text, at } => {
            ChatRow::new(ChatKind::Agent, key, agent)
                .time(Some(local(at)))
                .copy(text)
                .show(ui, style, scope, |ui, id| chat_bubble::markdown(ui, style, text, style.text, id));
        }
        Item::Reasoning(text) => {
            ChatRow::new(ChatKind::Reasoning, key, "Thinking")
                .copy(text)
                .summary(chat_bubble::summary_line(text, SUMMARY_CHARS))
                .show(ui, style, scope, |ui, id| chat_bubble::markdown(ui, style, text, style.text, id));
        }
        Item::ToolCall { name, arguments } => {
            ChatRow::new(ChatKind::Tool, key, chat_bubble::tool_label(name))
                .summary(chat_bubble::json_summary(arguments, SUMMARY_CHARS))
                .show(ui, style, scope, |ui, id| chat_bubble::json(ui, style, arguments, id));
        }
        Item::ToolOutput(text) => {
            ChatRow::new(ChatKind::Tool, key, "Result")
                .copy(text)
                .summary(chat_bubble::summary_line(text, SUMMARY_CHARS))
                .show(ui, style, scope, |ui, id| chat_bubble::payload(ui, style, text, style.text, id));
        }
    }
}

fn run_row(ui: &mut Ui, style: &ChatStyle, scope: Id, index: usize, run: &Run) {
    let key = format!("run{}", run.id);
    let outcome = run.outcome();
    let color = match outcome {
        Outcome::Ok => style.weak,
        Outcome::Degraded => theme::warn(ui),
        Outcome::Failed => style.error,
        Outcome::Skipped | Outcome::Unknown => style.weak,
    };
    let output = run.output.as_deref().unwrap_or_default();
    let mut row = ChatRow::new(ChatKind::Agent, &key, outcome.label())
        .time(Some(local(&run.started_at)))
        .copy(output)
        .has_body(!output.trim().is_empty())
        .default_open(index == 0);
    if let Some(ms) = run.duration_ms {
        row = row.badge(chat_bubble::duration_label(ms.max(0) as u64), color);
    }
    row.show(ui, style, scope, |ui, id| chat_bubble::markdown(ui, style, output, style.text, id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, agent: &str, messages: u64, at: &str) -> SessionRow {
        SessionRow {
            id: id.into(),
            agent: agent.into(),
            name: String::new(),
            messages,
            last_activity: at.into(),
            keys: vec![id.into()],
        }
    }

    fn job(id: &str) -> Automation {
        serde_json::from_value(serde_json::json!({"id": id})).expect("job")
    }

    #[test]
    fn sessions_group_by_agent_in_order_of_each_agents_newest_session() {
        let mut rows = vec![
            row("a", "sweeper", 2, "2026-09-10T06:19:00Z"),
            row("b", "tech_chat", 1, "2026-09-26T16:20:04.088140183+00:00"),
            row("c", "sweeper", 5, "2026-09-12T10:00:00Z"),
            row("d", "tech_chat", 3, "2026-09-08T14:29:00Z"),
            row("e", "consolidator", 1, "not a stamp"),
        ];
        newest_first(&mut rows);
        let order: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, ["b", "c", "a", "d", "e"]);
        let groups: Vec<(&str, Vec<&str>)> = group_by_agent(&rows)
            .into_iter()
            .map(|(agent, g)| (agent, g.iter().map(|r| r.id.as_str()).collect()))
            .collect();
        assert_eq!(groups, [("tech_chat", vec!["b", "d"]), ("sweeper", vec!["c", "a"]), ("consolidator", vec!["e"])]);
    }

    #[test]
    fn only_messages_after_the_baseline_count_as_new_until_read() {
        let mut view = ZeroClawView::default();
        view.sessions = vec![row("s", "tech_chat", 2, "2026-09-26T10:00:00Z")];
        assert!(!view.unread(&view.sessions[0]), "nothing is new before the baseline");
        view.baseline();
        assert!(!view.unread(&view.sessions[0]), "the first list is taken as read");

        view.sessions = vec![
            row("s", "tech_chat", 3, "2026-09-26T11:00:00Z"),
            row("cron_shelf_triage", "tech_chat", 1, "2026-09-26T16:20:00Z"),
        ];
        view.baseline();
        assert!(view.unread(&view.sessions[0]), "a new message in a known session");
        assert!(view.unread(&view.sessions[1]), "a session that appeared after the baseline");
        assert!(view.job_unread(&job("shelf_triage")), "an automation follows its result session");
        assert!(!view.job_unread(&job("bsod_sweep")), "an automation without a result session");
        assert_eq!(view.unread_count(), 2);

        view.mark_read("cron_shelf_triage");
        assert!(!view.job_unread(&job("shelf_triage")));
        assert!(view.unread(&view.sessions[0]));
        assert!(view.seen_dirty);
    }

    #[test]
    fn picking_an_automation_shows_its_result_session_and_drops_the_session_pick() {
        let mut view = ZeroClawView::default();
        view.jobs = vec![job("shelf_triage")];
        view.sessions = vec![row("cron_shelf_triage", "tech_chat", 1, "2026-09-26T16:20:00Z")];
        view.select(&ZeroClawPick::Session("other".into()));
        assert_eq!(view.picked(), Some(ZeroClawPick::Session("other".into())));
        view.select(&ZeroClawPick::Automation("shelf_triage".into()));
        assert_eq!(view.picked(), Some(ZeroClawPick::Automation("shelf_triage".into())));
        assert_eq!(view.shown_session().as_deref(), Some("cron_shelf_triage"));
        view.deselect();
        assert_eq!(view.picked(), None);
        assert_eq!(view.shown_session(), None);
    }

    #[test]
    fn a_search_matches_any_listed_field_case_insensitively() {
        assert!(mentions("", &["anything"]));
        assert!(mentions("shelf", &["Shelf triage", "tech_chat"]));
        assert!(mentions("tech", &["Shelf triage", "tech_chat"]));
        assert!(!mentions("sweep", &["Shelf triage", "tech_chat"]));
    }
}

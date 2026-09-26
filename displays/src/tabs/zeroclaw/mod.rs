//! Root-only view of the ZeroClaw gateway: each agent's sessions and transcripts, and the
//! scheduled automations with their runs, read from the same API the zc-codex app uses.

mod api;

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Local};
use crossbeam::channel::{Receiver, Sender};
use database::schema::ZeroclawGateway;
use eframe::egui::{self, Align, Color32, Id, Layout, RichText, ScrollArea, Ui};
use web_time::Instant;

use crate::ui_tools::chat_bubble::{self, ChatKind, ChatRow, ChatStyle};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use api::{Automation, Item, Outcome, Run, SessionRow};

/// Session and automation list poll while the tab is drawn.
const LIST_POLL: Duration = Duration::from_secs(30);
/// Transcript poll while its session has a turn running, and otherwise.
const TRANSCRIPT_POLL_BUSY: Duration = Duration::from_secs(5);
const TRANSCRIPT_POLL_IDLE: Duration = Duration::from_secs(60);
const RUNS_POLL: Duration = Duration::from_secs(30);
const RUNS_SHOWN: usize = 20;
const SUMMARY_CHARS: usize = 160;
const DENIED: &str = "Only an active Root user can browse ZeroClaw.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Pane {
    #[default]
    Sessions,
    Automations,
}

enum Msg {
    Gateway(Result<Option<ZeroclawGateway>, String>),
    Agents(Result<Vec<String>, String>),
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
    agents: Vec<String>,
    /// `None` shows every agent.
    agent: Option<String>,
    pane: Pane,
    sessions: Vec<SessionRow>,
    running: HashSet<String>,
    session: Option<String>,
    transcript: Vec<Item>,
    transcript_for: Option<String>,
    jobs: Vec<Automation>,
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
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Default for ZeroClawView {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            access: Access::Unknown,
            agents: Vec::new(),
            agent: None,
            pane: Pane::default(),
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

impl ZeroClawView {
    pub fn ui(&mut self, ui: &mut Ui) {
        self.drain();
        match &self.access {
            Access::Unknown => self.load_gateway(),
            Access::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting to ZeroClaw\u{2026}");
                });
            }
            Access::Denied => {
                ui.label(RichText::new(DENIED).color(theme::weak_text(ui)));
            }
            Access::Failed(why) => {
                let why = why.clone();
                ui.label(RichText::new(format!("{} {why}", icons::STATUS_ERR)).color(theme::error(ui)));
                if ui.button(format!("{} Retry", icons::REFRESH)).clicked() {
                    self.access = Access::Unknown;
                }
            }
            Access::Ready(_) => {
                self.poll();
                self.ready_ui(ui);
            }
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
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
                Msg::Agents(Ok(agents)) => self.agents = agents,
                Msg::Agents(Err(e)) => self.error = Some(e),
                Msg::Sessions(r) => {
                    self.loading_lists = false;
                    match r {
                        Ok((sessions, running)) => {
                            self.sessions = sessions;
                            self.running = running.into_iter().collect();
                            self.error = None;
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
            let want_agents = self.agents.is_empty();
            PlatformSpawner::spawn(async move {
                if want_agents {
                    let _ = tx.send(Msg::Agents(api::agents(&gw).await.map_err(|e| e.to_string())));
                }
                let _ = tx.send(Msg::Jobs(api::automations(&gw).await.map_err(|e| e.to_string())));
                let lists = async { Ok::<_, anyhow::Error>((api::sessions(&gw).await?, api::running(&gw).await.unwrap_or_default())) };
                let _ = tx.send(Msg::Sessions(lists.await.map_err(|e| e.to_string())));
            });
        }
        if let (Some(id), Some(gw)) = (self.session.clone(), self.gateway()) {
            let every = if self.running.contains(&id) { TRANSCRIPT_POLL_BUSY } else { TRANSCRIPT_POLL_IDLE };
            let stale = self.transcript_for.as_deref() != Some(id.as_str());
            if !self.loading_transcript && (stale || due(self.last_transcript, every))
                && let Some(row) = self.sessions.iter().find(|s| s.id == id).cloned()
            {
                self.loading_transcript = true;
                self.last_transcript = Some(Instant::now());
                let tx = self.tx.clone();
                PlatformSpawner::spawn(async move {
                    let _ = tx.send(Msg::Transcript(row.id.clone(), api::transcript(&gw, &row).await.map_err(|e| e.to_string())));
                });
            }
        }
        if let (Some(job), Some(gw)) = (self.job.clone(), self.gateway()) {
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

    fn ready_ui(&mut self, ui: &mut Ui) {
        egui::Panel::top("zeroclaw_bar").show_separator_line(false).show(ui, |ui| self.top_bar(ui));
        egui::Panel::left("zeroclaw_list")
            .resizable(true)
            .default_size(280.)
            .min_size(200.)
            .max_size(480.)
            .show(ui, |ui| match self.pane {
                Pane::Sessions => self.session_list(ui),
                Pane::Automations => self.automation_list(ui),
            });
        egui::CentralPanel::default().show(ui, |ui| match self.pane {
            Pane::Sessions => self.transcript_ui(ui),
            Pane::Automations => self.runs_ui(ui),
        });
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let shown = self.agent.clone().unwrap_or_else(|| "All agents".to_string());
            egui::ComboBox::from_id_salt("zeroclaw_agent")
                .selected_text(format!("{}  {shown}", icons::ROBOT))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(self.agent.is_none(), "All agents").clicked() {
                        self.agent = None;
                    }
                    for a in self.agents.clone() {
                        if ui.selectable_label(self.agent.as_deref() == Some(a.as_str()), &a).clicked() {
                            self.agent = Some(a);
                        }
                    }
                });
            ui.separator();
            ui.selectable_value(&mut self.pane, Pane::Sessions, format!("{} Sessions", icons::CHAT));
            let failing = self.visible_jobs().filter(|j| matches!(j.outcome(), Outcome::Failed | Outcome::Degraded)).count();
            let automations = if failing > 0 {
                format!("{} Automations ({failing} failing)", icons::AUTOMATION)
            } else {
                format!("{} Automations", icons::AUTOMATION)
            };
            ui.selectable_value(&mut self.pane, Pane::Automations, automations);
            ui.separator();
            if ui.button(icons::REFRESH).on_hover_text("Refresh now").clicked() {
                self.last_lists = None;
                self.last_transcript = None;
                self.last_runs = None;
            }
            if self.loading_lists || self.loading_transcript || self.loading_runs {
                ui.spinner();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(e) = &self.error {
                    ui.label(RichText::new(format!("{} {e}", icons::STATUS_ERR)).color(theme::error(ui)).small());
                } else if let Some(n) = &self.notice {
                    ui.label(RichText::new(n).color(theme::weak_text(ui)).small());
                }
            });
        });
    }

    fn visible_sessions(&self) -> impl Iterator<Item = &SessionRow> {
        self.sessions.iter().filter(|s| self.agent.as_deref().is_none_or(|a| s.agent == a))
    }

    fn visible_jobs(&self) -> impl Iterator<Item = &Automation> {
        self.jobs.iter().filter(|j| self.agent.as_deref().is_none_or(|a| j.agent_alias == a))
    }

    fn session_list(&mut self, ui: &mut Ui) {
        let rows: Vec<SessionRow> = self.visible_sessions().cloned().collect();
        if rows.is_empty() {
            ui.label(RichText::new("No sessions").color(theme::weak_text(ui)));
            return;
        }
        ScrollArea::vertical().id_salt("zeroclaw_sessions").auto_shrink([false, false]).show(ui, |ui| {
            for row in rows {
                let selected = self.session.as_deref() == Some(row.id.as_str());
                let icon = if row.is_automation() { icons::AUTOMATION } else { icons::CHAT };
                let resp = ui
                    .horizontal(|ui| {
                        if self.running.contains(&row.id) {
                            ui.spinner();
                        }
                        ui.selectable_label(selected, format!("{icon}  {}", row.label()))
                    })
                    .inner
                    .on_hover_text(format!("{} \u{00b7} {}", row.agent, row.id));
                ui.label(
                    RichText::new(format!("{} \u{00b7} {} msg \u{00b7} {}", row.agent, row.messages, local(&row.last_activity)))
                        .small()
                        .color(theme::weak_text(ui)),
                );
                if resp.clicked() && !selected {
                    self.session = Some(row.id.clone());
                    self.transcript.clear();
                    self.transcript_for = None;
                    self.error = None;
                }
                ui.add_space(4.);
            }
        });
    }

    fn transcript_ui(&mut self, ui: &mut Ui) {
        let Some(row) = self.session.as_ref().and_then(|id| self.sessions.iter().find(|s| &s.id == id)).cloned() else {
            ui.label(RichText::new("Pick a session to read its transcript.").color(theme::weak_text(ui)));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(row.label()).strong());
            ui.label(RichText::new(format!("\u{00b7} {} \u{00b7} {}", row.agent, row.id)).color(theme::weak_text(ui)));
            if self.running.contains(&row.id) {
                ui.label(RichText::new("\u{00b7} working").color(theme::accent(ui)));
            }
        });
        ui.separator();
        if self.transcript_for.as_deref() != Some(row.id.as_str()) {
            ui.spinner();
            return;
        }
        let style = ChatStyle::from_ui(ui);
        let scope = Id::new(("zeroclaw_transcript", row.id.as_str()));
        let agent = row.agent.clone();
        ScrollArea::vertical()
            .id_salt(("zeroclaw_transcript_scroll", row.id.as_str()))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (i, item) in self.transcript.iter().enumerate() {
                    item_row(ui, &style, scope, &format!("{i}"), &agent, item);
                }
            });
    }

    fn automation_list(&mut self, ui: &mut Ui) {
        let jobs: Vec<Automation> = self.visible_jobs().cloned().collect();
        if jobs.is_empty() {
            ui.label(RichText::new("No automations").color(theme::weak_text(ui)));
            return;
        }
        ScrollArea::vertical().id_salt("zeroclaw_jobs").auto_shrink([false, false]).show(ui, |ui| {
            for job in jobs {
                let selected = self.job.as_deref() == Some(job.id.as_str());
                let (icon, color) = outcome_mark(ui, job.outcome());
                let resp = ui
                    .horizontal(|ui| {
                        if self.starting.contains(&job.id) {
                            ui.spinner();
                        } else {
                            ui.label(RichText::new(icon).color(color));
                        }
                        ui.selectable_label(selected, job.label())
                    })
                    .inner
                    .on_hover_text(format!("{} \u{00b7} {}", job.agent_alias, job.id));
                let last = job.last_run.as_deref().map(local).unwrap_or_else(|| "never".to_string());
                let next = job.next_run.as_deref().map(local).unwrap_or_else(|| "\u{2014}".to_string());
                let state = if job.enabled { "" } else { " \u{00b7} paused" };
                ui.label(
                    RichText::new(format!("{} \u{00b7} last {last} \u{00b7} next {next}{state}", job.expression))
                        .small()
                        .color(theme::weak_text(ui)),
                );
                if resp.clicked() && !selected {
                    self.job = Some(job.id.clone());
                    self.runs.clear();
                    self.runs_for = None;
                    self.error = None;
                }
                ui.add_space(4.);
            }
        });
    }

    fn runs_ui(&mut self, ui: &mut Ui) {
        let Some(job) = self.job.as_ref().and_then(|id| self.jobs.iter().find(|j| &j.id == id)).cloned() else {
            ui.label(RichText::new("Pick an automation to read its runs.").color(theme::weak_text(ui)));
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

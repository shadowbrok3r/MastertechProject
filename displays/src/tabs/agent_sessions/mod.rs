//! Browses ZeroClaw agent turns as readable transcripts.

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea, Ui, vec2};

use crate::ai::zeroclaw_sessions::{AgentSession, Entry};
use crate::markdown_editor::chat_markdown;
use crate::ui_tools::hex_json;
use crate::{PlatformSpawner, Spawner};

const AUTO_REFRESH_SECS: u64 = 20;

pub struct AgentSessions {
    sessions: Vec<AgentSession>,
    selected: Option<String>,
    filter: String,
    failures_only: bool,
    auto_refresh: bool,
    last_poll: Option<Instant>,
    loading: bool,
    status: String,
    tx: Sender<Result<Vec<AgentSession>, String>>,
    rx: Receiver<Result<Vec<AgentSession>, String>>,
}

impl Default for AgentSessions {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            sessions: Vec::new(),
            selected: None,
            filter: String::new(),
            failures_only: false,
            auto_refresh: true,
            last_poll: None,
            loading: false,
            status: String::new(),
            tx,
            rx,
        }
    }
}

impl AgentSessions {
    fn refresh(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.last_poll = Some(Instant::now());
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let _ = tx.send(crate::ai::zeroclaw_sessions::fetch().await.map_err(|e| e.to_string()));
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.loading = false;
            match msg {
                Ok(sessions) => {
                    self.status = format!("{} turns", sessions.len());
                    if self.selected.is_none() {
                        self.selected = sessions.first().map(|s| s.trace_id.clone());
                    }
                    self.sessions = sessions;
                }
                Err(e) => self.status = e,
            }
        }
    }

    fn visible(&self) -> Vec<&AgentSession> {
        let needle = self.filter.trim().to_lowercase();
        self.sessions
            .iter()
            .filter(|s| !self.failures_only || s.failures > 0)
            .filter(|s| {
                needle.is_empty()
                    || s.agent.to_lowercase().contains(&needle)
                    || s.trace_id.contains(&needle)
                    || s.entries.iter().any(|(_, e)| entry_text(e).to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.drain();
        if self.last_poll.is_none() {
            self.refresh();
        }
        if self.auto_refresh
            && !self.loading
            && self.last_poll.is_some_and(|t| t.elapsed() >= Duration::from_secs(AUTO_REFRESH_SECS))
        {
            self.refresh();
        }
        if self.loading {
            ui.ctx().request_repaint();
        } else if self.auto_refresh {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
            ui.checkbox(&mut self.auto_refresh, "Auto");
            ui.checkbox(&mut self.failures_only, "Failures only");
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(220.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if self.loading { "loading...".to_string() } else { self.status.clone() };
                ui.label(RichText::new(text).weak());
            });
        });
        ui.separator();

        let list_w = (ui.available_width() * 0.30).clamp(220.0, 380.0);
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(list_w, height), Layout::top_down(Align::Min), |ui| {
                self.list_ui(ui);
            });
            ui.separator();
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::top_down(Align::Min),
                |ui| self.transcript_ui(ui),
            );
        });
    }

    fn list_ui(&mut self, ui: &mut Ui) {
        let visible: Vec<(String, String)> = self
            .visible()
            .iter()
            .map(|s| (s.trace_id.clone(), summary_line(s)))
            .collect();
        if visible.is_empty() {
            ui.label(RichText::new("No turns match.").weak());
            return;
        }
        ScrollArea::vertical().id_salt("agent_session_list").show(ui, |ui| {
            for (trace_id, summary) in visible {
                let selected = self.selected.as_deref() == Some(trace_id.as_str());
                let failed = self
                    .sessions
                    .iter()
                    .find(|s| s.trace_id == trace_id)
                    .is_some_and(|s| s.failures > 0);
                let mut text = RichText::new(summary);
                if failed {
                    text = text.color(Color32::from_rgb(220, 120, 120));
                }
                if ui.selectable_label(selected, text).clicked() {
                    self.selected = Some(trace_id.clone());
                }
            }
        });
    }

    fn transcript_ui(&mut self, ui: &mut Ui) {
        let Some(session) = self
            .selected
            .as_ref()
            .and_then(|id| self.sessions.iter().find(|s| &s.trace_id == id))
        else {
            ui.label(RichText::new("Select a turn.").weak());
            return;
        };

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&session.agent).strong());
            ui.label(RichText::new(format!("· {}", session.model)).weak());
            ui.label(RichText::new(format!("· {}", session.short_id())).weak());
            ui.label(RichText::new(format!("· {} tools", session.tool_calls())).weak());
            if session.failures > 0 {
                ui.label(
                    RichText::new(format!("· {} failed", session.failures))
                        .color(Color32::from_rgb(220, 120, 120)),
                );
            }
            if session.input_tokens + session.output_tokens > 0 {
                ui.label(
                    RichText::new(format!(
                        "· {}in/{}out",
                        session.input_tokens, session.output_tokens
                    ))
                    .weak(),
                );
            }
            if session.cost_usd > 0.0 {
                ui.label(RichText::new(format!("· ${:.4}", session.cost_usd)).weak());
            }
        });
        ui.label(RichText::new(format!("{} - {}", session.started, session.ended)).weak());
        ui.separator();

        ScrollArea::vertical().id_salt("agent_session_body").show(ui, |ui| {
            for (idx, (ts, entry)) in session.entries.iter().enumerate() {
                entry_ui(ui, &session.trace_id, idx, ts, entry);
            }
        });
    }
}

fn summary_line(s: &AgentSession) -> String {
    let clock = s.ended.split('T').nth(1).map(|t| &t[..t.len().min(8)]).unwrap_or("");
    let head = s
        .outcome()
        .map(|t| t.lines().next().unwrap_or("").chars().take(48).collect::<String>())
        .unwrap_or_default();
    format!("{clock}  {}  ({})\n{head}", s.agent, s.short_id())
}

fn entry_text(e: &Entry) -> &str {
    match e {
        Entry::Prompt { text, .. } | Entry::Response { text } | Entry::Final { text } => text,
        Entry::ToolCall { tool, .. } | Entry::ToolResult { tool, .. } => tool,
    }
}

/// Renders `body` as a JSON tree when it parses, else as plain text.
fn payload_ui(ui: &mut Ui, id_salt: &str, body: &str) {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) if v.is_object() || v.is_array() => hex_json::json_tree(ui, id_salt, &v),
        _ => {
            ui.label(RichText::new(body).monospace());
        }
    }
}

fn entry_ui(ui: &mut Ui, trace: &str, idx: usize, ts: &str, entry: &Entry) {
    let clock = ts.split('T').nth(1).map(|t| &t[..t.len().min(12)]).unwrap_or(ts);
    let salt = format!("{trace}:{idx}");
    match entry {
        Entry::Response { text } | Entry::Final { text } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(entry.label()).strong());
                ui.label(RichText::new(clock).weak().small());
            });
            chat_markdown::render(ui, text);
            ui.add_space(6.0);
        }
        Entry::Prompt { text, truncated } => {
            let title = if *truncated {
                format!("{} · {clock} · truncated", entry.label())
            } else {
                format!("{} · {clock}", entry.label())
            };
            egui::CollapsingHeader::new(RichText::new(title).weak())
                .id_salt(&salt)
                .default_open(false)
                .show(ui, |ui| payload_ui(ui, &salt, text));
        }
        Entry::ToolCall { tool, arguments } => {
            egui::CollapsingHeader::new(RichText::new(format!("{tool} · {clock}")).strong())
                .id_salt(&salt)
                .default_open(false)
                .show(ui, |ui| payload_ui(ui, &salt, arguments));
        }
        Entry::ToolResult { tool, output, error } => {
            let title = match error {
                Some(e) => RichText::new(format!("{tool} failed · {e}")).color(Color32::from_rgb(220, 120, 120)),
                None => RichText::new(format!("{tool} result · {clock}")).weak(),
            };
            egui::CollapsingHeader::new(title)
                .id_salt(&salt)
                .default_open(error.is_some())
                .show(ui, |ui| payload_ui(ui, &salt, output));
        }
    }
}

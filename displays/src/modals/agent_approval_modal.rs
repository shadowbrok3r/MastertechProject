//! Modal for decisions a Codex agent is waiting on: permission to run a
//! machine-touching tool, or a question only the technician can answer.
//!
//! The admin-agent broker writes `agent_approval` rows and polls them; this
//! modal shows the front pending row for the signed-in tech (their own, or
//! their store's; Root sees all) and records the decision with a conditional
//! UPDATE, so two consoles clicking at once produce one winner and one stale
//! click. Snoozing hides a row here only; it stays pending for everyone else.

use std::collections::HashMap;

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{AgentApproval, AgentDecideOutcome, RecordId, RecordIdExt};
use eframe::egui::{self, Align, Grid, Id, Layout, Modal, RichText, ScrollArea, TextEdit};
use serde_json::{json, Value};

use crate::ui_tools::{hex_json, icons, theme};
use crate::{PlatformSpawner, Spawner};

/// Seconds between snapshot polls of the pending set.
const POLL_SECS: f64 = 3.0;
/// Seconds a row stays hidden after "Later".
const SNOOZE_SECS: f64 = 90.0;

enum Msg {
    Snapshot(Result<Vec<AgentApproval>, String>),
    Done(RecordId),
    Stale(RecordId, String),
    Failed(RecordId, String),
}

#[derive(Default)]
pub struct AgentApprovalQueue {
    pending: Vec<AgentApproval>,
    in_flight: Vec<RecordId>,
    resolved: Vec<RecordId>,
    snoozed: HashMap<RecordId, f64>,
    deny_note: String,
    /// Draft answers keyed by question id.
    answers: HashMap<String, String>,
    last_poll: Option<f64>,
    polling: bool,
    last_error: Option<String>,
    chan: Option<(Sender<Msg>, Receiver<Msg>)>,
}

impl AgentApprovalQueue {
    fn chan(&mut self) -> &(Sender<Msg>, Receiver<Msg>) {
        self.chan.get_or_insert_with(unbounded)
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    fn poll(&mut self, now: f64) {
        if self.polling || self.last_poll.is_some_and(|t| now - t < POLL_SECS) {
            return;
        }
        let Some(user) = crate::get_current_user_from_auth() else { return };
        self.polling = true;
        self.last_poll = Some(now);
        let tx = self.chan().0.clone();
        let root = user.get_authorization() == database::schema::user::UserAuthorization::Root;
        let id = user.get_id();
        let store = serde_json::to_value(&user)
            .ok()
            .and_then(|v| v.get("store").and_then(Value::as_str).map(str::to_string));
        PlatformSpawner::spawn(async move {
            let rows = if root {
                AgentApproval::list_pending().await
            } else {
                AgentApproval::list_pending_for(&id, store.as_deref()).await
            };
            let _ = tx.try_send(Msg::Snapshot(rows.map_err(|e| e.to_string())));
        });
    }

    fn drain(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = self.chan.as_ref() else { return };
        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() {
            msgs.push(m);
        }
        for msg in msgs {
            ctx.request_repaint();
            match msg {
                Msg::Snapshot(Ok(rows)) => {
                    self.polling = false;
                    self.pending = rows
                        .into_iter()
                        .filter(|r| r.is_pending() && !self.resolved.contains(&r.id))
                        .collect();
                }
                Msg::Snapshot(Err(e)) => {
                    self.polling = false;
                    log::warn!("agent_approval snapshot failed: {e}");
                }
                Msg::Done(id) => self.remove(&id),
                Msg::Stale(id, what) => {
                    self.remove(&id);
                    let _ = crate::get_toast_sender()
                        .try_send(crate::ToastMessage::Warning(format!("Agent request was already {what}.")));
                }
                Msg::Failed(id, err) => {
                    self.in_flight.retain(|p| p != &id);
                    self.last_error = Some(err);
                }
            }
        }
    }

    fn remove(&mut self, id: &RecordId) {
        if self.pending.first().map(|r| &r.id) == Some(id) {
            self.deny_note.clear();
            self.answers.clear();
        }
        self.pending.retain(|r| &r.id != id);
        self.in_flight.retain(|p| p != id);
        self.snoozed.remove(id);
        if !self.resolved.contains(id) {
            self.resolved.push(id.clone());
            if self.resolved.len() > 64 {
                self.resolved.remove(0);
            }
        }
    }

    fn decide(&mut self, id: RecordId, status: &str, note: Option<String>, answers: Option<Value>) {
        self.in_flight.push(id.clone());
        self.last_error = None;
        let tx = self.chan().0.clone();
        let decided_by = crate::get_current_user_from_auth().map(|u| u.get_id());
        let status = status.to_string();
        PlatformSpawner::spawn(async move {
            let msg = match AgentApproval::decide(&id, &status, decided_by, note, answers).await {
                Ok(AgentDecideOutcome::Recorded) => Msg::Done(id),
                Ok(AgentDecideOutcome::AlreadyResolved(held)) => Msg::Stale(id, held),
                Ok(AgentDecideOutcome::Missing) => Msg::Stale(id, "withdrawn".into()),
                Err(e) => Msg::Failed(id, e.to_string()),
            };
            let _ = tx.try_send(msg);
        });
    }

    /// Polls, drains and draws; call once per frame from the shared receive loop.
    pub fn tick_and_ui(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.drain(ctx);
        self.poll(now);
        let Some(req) = self
            .pending
            .iter()
            .find(|r| !self.snoozed.get(&r.id).is_some_and(|until| *until > now) && r.secs_remaining() > 0)
            .cloned()
        else {
            return;
        };
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let busy = self.in_flight.contains(&req.id);
        let queued = self.pending.len();
        let mut decision: Option<(&'static str, Option<String>, Option<Value>)> = None;
        let mut later = false;

        Modal::new(Id::new("agent_approval_modal")).show(ctx, |ui| {
            ui.set_width(640.0);
            let is_question = req.kind == "question";
            ui.horizontal(|ui| {
                let heading = if is_question {
                    format!("{} The AI agent has a question", icons::CHAT)
                } else {
                    format!("{} The AI agent needs approval", icons::LOCK)
                };
                ui.heading(heading);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if queued > 1 {
                        ui.label(RichText::new(format!("{queued} waiting")).color(theme::warn(ui)).strong());
                    }
                });
            });
            ui.separator();

            Grid::new("agent_approval_meta").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                if let Some(cs) = req.connection_string.as_deref() {
                    ui.label(RichText::new("Machine").strong());
                    ui.label(cs);
                    ui.end_row();
                }
                if let Some(tool) = req.tool.as_deref().filter(|_| !is_question) {
                    ui.label(RichText::new("Tool").strong());
                    ui.label(RichText::new(tool).monospace());
                    ui.end_row();
                }
                ui.label(RichText::new("Expires in").strong());
                let secs = req.secs_remaining();
                let color = if secs < 60 { theme::error(ui) } else { theme::weak_text(ui) };
                ui.label(RichText::new(format!("{}m {:02}s", secs / 60, secs % 60)).color(color));
                ui.end_row();
            });

            ui.add_space(6.0);
            if is_question {
                self.question_body(ui, &req);
            } else {
                ui.label(RichText::new(&req.summary).strong());
                if let Some(args) = req.arguments.as_ref().filter(|a| a.as_object().is_some_and(|o| !o.is_empty())) {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Arguments").strong().small());
                    ScrollArea::vertical().max_height(220.0).id_salt(("approval_args", req.id.key_string())).show(ui, |ui| {
                        hex_json::json_tree(ui, &format!("approval:{}", req.id.key_string()), args);
                    });
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Note to the agent (optional, sent when you decline)").strong().small());
                ui.add(TextEdit::singleline(&mut self.deny_note).desired_width(f32::INFINITY));
            }

            if let Some(err) = self.last_error.as_ref() {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{} {err}", icons::STATUS_ERR)).color(theme::error(ui)));
            }

            ui.add_space(10.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if is_question {
                        if ui.button(RichText::new(format!("{} Answer", icons::CHECK)).color(theme::success(ui))).clicked() {
                            decision = Some(("answered", None, Some(self.collected_answers(&req))));
                        }
                        ui.add_space(6.0);
                        if ui.button(RichText::new(format!("{} Skip", icons::CLOSE)).color(theme::warn(ui))).clicked() {
                            decision = Some(("declined", None, None));
                        }
                    } else {
                        if ui.button(RichText::new(format!("{} Approve once", icons::STATUS_ON)).color(theme::success(ui))).clicked() {
                            decision = Some(("accepted", None, None));
                        }
                        if req.may_approve_for_session() {
                            ui.add_space(4.0);
                            if ui.button(format!("{} Approve for this session", icons::CHECK)).clicked() {
                                decision = Some(("accepted_for_session", None, None));
                            }
                        }
                        if req.may_approve_all() {
                            ui.add_space(4.0);
                            let label = RichText::new(format!("{} Approve ALL for this session", icons::APPROVE_ALL))
                                .color(theme::warn(ui))
                                .strong();
                            if ui
                                .button(label)
                                .on_hover_text("Run every tool call this agent makes on this machine without asking, until the session closes or you turn prompts back on")
                                .clicked()
                            {
                                decision = Some((database::schema::agent_approval::ACCEPTED_ALL_FOR_SESSION, None, None));
                            }
                        }
                        ui.add_space(6.0);
                        if ui.button(RichText::new(format!("{} Decline", icons::STATUS_ERR)).color(theme::error(ui))).clicked() {
                            let note = (!self.deny_note.trim().is_empty()).then(|| self.deny_note.trim().to_string());
                            decision = Some(("declined", note, None));
                        }
                        ui.add_space(4.0);
                        if ui.button(RichText::new(format!("{} Stop the agent", icons::STOP)).color(theme::error(ui))).clicked() {
                            decision = Some(("cancelled", None, None));
                        }
                    }
                    ui.add_space(8.0);
                    if ui.button("Later").clicked() {
                        later = true;
                    }
                });
                if busy {
                    ui.add_space(8.0);
                    ui.spinner();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(req.id.key_string()).small().color(theme::weak_text(ui)));
                });
            });
        });

        if later {
            self.snoozed.insert(req.id.clone(), now + SNOOZE_SECS);
            self.deny_note.clear();
            self.answers.clear();
        } else if let Some((status, note, answers)) = decision {
            self.deny_note.clear();
            self.answers.clear();
            self.decide(req.id.clone(), status, note, answers);
        }
    }

    /// The agent's questions with option pickers and a free-text field each.
    fn question_body(&mut self, ui: &mut egui::Ui, req: &AgentApproval) {
        let questions = req.questions.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
        if questions.is_empty() {
            ui.label(RichText::new(&req.summary).strong());
            let draft = self.answers.entry("answer".into()).or_default();
            ui.add(TextEdit::multiline(draft).desired_rows(3).desired_width(f32::INFINITY));
            return;
        }
        ScrollArea::vertical().max_height(320.0).id_salt(("approval_questions", req.id.key_string())).show(ui, |ui| {
            for (n, q) in questions.iter().enumerate() {
                let qid = q.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("q{n}"));
                if let Some(header) = q.get("header").and_then(Value::as_str) {
                    ui.label(RichText::new(header).strong().small());
                }
                ui.label(q.get("question").and_then(Value::as_str).unwrap_or(""));
                let options = q.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
                let draft = self.answers.entry(qid.clone()).or_default();
                for opt in &options {
                    let label = opt.get("label").and_then(Value::as_str).unwrap_or("");
                    let description = opt.get("description").and_then(Value::as_str).unwrap_or("");
                    let chosen = draft == label;
                    let text = if description.is_empty() { label.to_string() } else { format!("{label} — {description}") };
                    if ui.selectable_label(chosen, text).clicked() {
                        *draft = label.to_string();
                    }
                }
                let free_text = options.is_empty() || q.get("isOther").and_then(Value::as_bool).unwrap_or(true);
                if free_text {
                    let secret = q.get("isSecret").and_then(Value::as_bool).unwrap_or(false);
                    let edit = TextEdit::singleline(draft).desired_width(f32::INFINITY).hint_text("Type an answer…");
                    ui.add(if secret { edit.password(true) } else { edit });
                }
                ui.add_space(6.0);
            }
        });
    }

    /// `{qid: [answer]}`; the broker turns it into codex's shape.
    fn collected_answers(&self, req: &AgentApproval) -> Value {
        let mut out = serde_json::Map::new();
        let questions = req.questions.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
        if questions.is_empty() {
            let text = self.answers.get("answer").cloned().unwrap_or_default();
            out.insert("answer".into(), json!([text]));
            return Value::Object(out);
        }
        for (n, q) in questions.iter().enumerate() {
            let qid = q.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("q{n}"));
            let text = self.answers.get(&qid).cloned().unwrap_or_default();
            out.insert(qid, json!([text]));
        }
        Value::Object(out)
    }
}

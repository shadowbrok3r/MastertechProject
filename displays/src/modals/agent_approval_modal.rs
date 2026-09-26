//! Decision modal for the session's technician; Root users get a toast that opens it.

use std::collections::{HashMap, HashSet};

use crossbeam::channel::{unbounded, Receiver, Sender};
use database::schema::{
    AgentApproval, AgentDecideOutcome, ApprovalAudience, ApprovalViewer, RecordId, RecordIdExt, User,
    AGENT_APPROVAL_TABLE,
};
use eframe::egui::{self, Align, Grid, Id, Layout, Modal, RichText, ScrollArea, TextEdit};
use serde_json::{json, Value};

use crate::modals::approval_toast::{self, ApprovalNotice, NoticeKind};
use crate::ui_tools::toasts::Toasts;
use crate::ui_tools::{hex_json, icons, theme};
use crate::{PlatformSpawner, Spawner};

/// Seconds between snapshot polls of the pending set.
const POLL_SECS: f64 = 3.0;
/// Seconds a row stays hidden after "Later".
const SNOOZE_SECS: f64 = 90.0;
const NOT_PERMITTED: &str = "Only this session's technician or a Root user can decide this request.";
const REMOTE_REFUSED: &str = "Decisions from a remote viewer are ignored; decide on your own Mastertech.";

enum Msg {
    Snapshot { viewer: ApprovalViewer, seq: u64, rows: Result<Vec<AgentApproval>, String> },
    Done(RecordId),
    Stale(RecordId, String),
    NotPermitted(RecordId),
    Failed(RecordId, String),
}

/// `rows` split into the viewer's modal rows and toast rows; rows hidden from the viewer are dropped.
fn split(rows: Vec<AgentApproval>, viewer: Option<&ApprovalViewer>) -> (Vec<AgentApproval>, Vec<AgentApproval>) {
    let mut modal = Vec::new();
    let mut toast = Vec::new();
    for row in rows {
        match row.audience_for(viewer) {
            ApprovalAudience::Modal => modal.push(row),
            ApprovalAudience::Toast => toast.push(row),
            ApprovalAudience::Hidden => {}
        }
    }
    (modal, toast)
}

/// The toast content for a row owned by another technician, or by no one.
fn notice_for(row: &AgentApproval) -> ApprovalNotice {
    ApprovalNotice {
        id: row.id.clone(),
        kind: if row.kind == "question" { NoticeKind::Question } else { NoticeKind::Tool },
        summary: row.summary.clone(),
        machine: row.connection_string.clone(),
        unowned: row.assignee.is_none(),
    }
}

#[derive(Default)]
pub struct AgentApprovalQueue {
    /// Pending rows the viewer decides in the modal.
    pending: Vec<AgentApproval>,
    /// Pending rows the viewer sees as toasts.
    others: Vec<AgentApproval>,
    in_flight: Vec<RecordId>,
    /// Rows the viewer decided; a snapshot already in flight cannot bring them back.
    resolved: Vec<RecordId>,
    /// Rows a refused click removed, hidden from snapshots issued up to the recorded poll.
    stale: HashMap<RecordId, u64>,
    snoozed: HashMap<RecordId, f64>,
    /// Toast rows the viewer dismissed.
    dismissed: HashSet<RecordId>,
    /// Toast rows with a toast posted.
    toasted: HashSet<RecordId>,
    /// The toast row the viewer opened with Review.
    summoned: Option<RecordId>,
    /// The row the modal last drew.
    shown: Option<RecordId>,
    viewer: Option<ApprovalViewer>,
    /// The last signed-in user, kept through sign-in gaps.
    last_user: Option<RecordId>,
    deny_note: String,
    /// Draft answers keyed by question id.
    answers: HashMap<String, String>,
    last_poll: Option<f64>,
    poll_seq: u64,
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

    /// Follows sign-in changes; snoozes and dismissals survive a gap but not a different user.
    fn set_viewer(&mut self, next: Option<ApprovalViewer>) {
        if next == self.viewer {
            return;
        }
        self.retire_toasts();
        self.pending.clear();
        self.others.clear();
        self.in_flight.clear();
        self.summoned = None;
        self.last_poll = None;
        self.last_error = None;
        if let Some(v) = &next
            && self.last_user.as_ref() != Some(&v.id)
        {
            self.snoozed.clear();
            self.dismissed.clear();
            self.resolved.clear();
            self.stale.clear();
            approval_toast::reset(AGENT_APPROVAL_TABLE);
            self.last_user = Some(v.id.clone());
        }
        self.viewer = next;
    }

    fn retire_toasts(&mut self) {
        for id in self.toasted.drain() {
            approval_toast::retire(&id);
        }
    }

    fn poll(&mut self, now: f64) {
        let Some(viewer) = self.viewer.clone() else { return };
        if self.polling || self.last_poll.is_some_and(|t| now - t < POLL_SECS) {
            return;
        }
        self.polling = true;
        self.last_poll = Some(now);
        self.poll_seq += 1;
        let seq = self.poll_seq;
        let tx = self.chan().0.clone();
        PlatformSpawner::spawn(async move {
            let rows = if viewer.root {
                AgentApproval::list_pending().await
            } else {
                AgentApproval::list_pending_mine().await
            };
            let _ = tx.try_send(Msg::Snapshot { viewer, seq, rows: rows.map_err(|e| e.to_string()) });
        });
    }

    /// Replaces both lists from a snapshot issued as poll `seq`.
    fn apply_snapshot(&mut self, rows: Vec<AgentApproval>, seq: u64) {
        self.stale.retain(|_, at| *at >= seq);
        let live: Vec<AgentApproval> = rows
            .into_iter()
            .filter(|r| r.is_pending() && !self.resolved.contains(&r.id) && !self.stale.contains_key(&r.id))
            .collect();
        let (modal, toast) = split(live, self.viewer.as_ref());
        self.pending = modal;
        self.others = toast;
        let others = &self.others;
        self.dismissed.retain(|id| others.iter().any(|r| &r.id == id));
    }

    fn drain(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = self.chan.as_ref() else { return };
        let msgs: Vec<Msg> = rx.try_iter().collect();
        for msg in msgs {
            ctx.request_repaint();
            match msg {
                Msg::Snapshot { viewer, seq, rows } => {
                    self.polling = false;
                    if self.viewer.as_ref() != Some(&viewer) {
                        continue;
                    }
                    match rows {
                        Ok(rows) => self.apply_snapshot(rows, seq),
                        Err(e) => log::warn!("agent_approval snapshot failed: {e}"),
                    }
                }
                Msg::Done(id) => self.remove(&id, true),
                Msg::Stale(id, what) => {
                    self.remove(&id, false);
                    let _ = crate::get_toast_sender()
                        .try_send(crate::ToastMessage::Warning(format!("Agent request was already {what}.")));
                }
                Msg::NotPermitted(id) => {
                    self.remove(&id, false);
                    let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(NOT_PERMITTED.into()));
                }
                Msg::Failed(id, err) => {
                    self.in_flight.retain(|p| p != &id);
                    self.last_error = Some(err);
                }
            }
        }
    }

    /// Drops a row; only a recorded decision keeps it out of later snapshots for good.
    fn remove(&mut self, id: &RecordId, recorded: bool) {
        if self.shown.as_ref() == Some(id) {
            self.deny_note.clear();
            self.answers.clear();
        }
        self.pending.retain(|r| &r.id != id);
        self.others.retain(|r| &r.id != id);
        self.in_flight.retain(|p| p != id);
        self.snoozed.remove(id);
        if self.toasted.remove(id) {
            approval_toast::retire(id);
        }
        if self.summoned.as_ref() == Some(id) {
            self.summoned = None;
        }
        if recorded {
            if !self.resolved.contains(id) {
                self.resolved.push(id.clone());
                if self.resolved.len() > 64 {
                    self.resolved.remove(0);
                }
            }
        } else {
            self.stale.insert(id.clone(), self.poll_seq);
        }
    }

    /// Takes toast clicks, retires toasts of resolved or lapsed rows and posts toasts for new ones.
    fn sync_toasts(&mut self, toasts: &mut Toasts) {
        for id in approval_toast::take_dismissed(AGENT_APPROVAL_TABLE) {
            self.toasted.remove(&id);
            self.dismissed.insert(id);
        }
        if let Some(id) = approval_toast::take_review(AGENT_APPROVAL_TABLE) {
            self.toasted.remove(&id);
            self.summoned = Some(id);
        }
        let live: HashSet<RecordId> =
            self.others.iter().filter(|r| r.secs_remaining() > 0).map(|r| r.id.clone()).collect();
        if self.summoned.as_ref().is_some_and(|id| !live.contains(id)) {
            self.summoned = None;
        }
        for id in self.toasted.iter().filter(|id| !live.contains(*id)) {
            approval_toast::retire(id);
        }
        self.toasted.retain(|id| live.contains(id));
        for row in &self.others {
            let skip = !live.contains(&row.id)
                || self.toasted.contains(&row.id)
                || self.dismissed.contains(&row.id)
                || self.summoned.as_ref() == Some(&row.id);
            if !skip && approval_toast::post(toasts, notice_for(row)) {
                self.toasted.insert(row.id.clone());
            }
        }
    }

    /// The row to draw and whether it came from a toast: a summoned row first, else the oldest unsnoozed modal row.
    fn front(&self, now: f64) -> Option<(AgentApproval, bool)> {
        let summoned = self
            .summoned
            .as_ref()
            .and_then(|id| self.others.iter().find(|r| &r.id == id && r.secs_remaining() > 0));
        if let Some(row) = summoned {
            return Some((row.clone(), true));
        }
        self.pending
            .iter()
            .find(|r| !self.snoozed.get(&r.id).is_some_and(|until| *until > now) && r.secs_remaining() > 0)
            .map(|r| (r.clone(), false))
    }

    fn decide(&mut self, req: &AgentApproval, status: &str, note: Option<String>, answers: Option<Value>) {
        if req.audience_for(self.viewer.as_ref()) == ApprovalAudience::Hidden {
            self.last_error = Some(NOT_PERMITTED.into());
            return;
        }
        self.in_flight.push(req.id.clone());
        self.last_error = None;
        let tx = self.chan().0.clone();
        let id = req.id.clone();
        let status = status.to_string();
        PlatformSpawner::spawn(async move {
            let msg = match AgentApproval::decide(&id, &status, note, answers).await {
                Ok(AgentDecideOutcome::Recorded) => Msg::Done(id),
                Ok(AgentDecideOutcome::AlreadyResolved(held)) => Msg::Stale(id, held),
                Ok(AgentDecideOutcome::Missing) => {
                    Msg::Stale(id, "withdrawn, or you are not permitted to decide it".into())
                }
                Ok(AgentDecideOutcome::NotPermitted) => Msg::NotPermitted(id),
                Err(e) => Msg::Failed(id, e.to_string()),
            };
            let _ = tx.try_send(msg);
        });
    }

    /// Polls, drains, posts Root toasts and draws; call once per frame from the shared receive loop.
    pub fn tick_and_ui(&mut self, ctx: &egui::Context, user: Option<&User>, toasts: &mut Toasts) {
        self.set_viewer(user.and_then(ApprovalViewer::of));
        let now = ctx.input(|i| i.time);
        self.drain(ctx);
        self.poll(now);
        self.sync_toasts(toasts);
        if !self.toasted.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        let Some((req, summoned)) = self.front(now) else {
            self.shown = None;
            return;
        };
        if self.shown.as_ref() != Some(&req.id) {
            self.deny_note.clear();
            self.answers.clear();
            self.shown = Some(req.id.clone());
        }
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
                if summoned {
                    ui.label(RichText::new("Deciding as").strong());
                    let whose = if req.assignee.is_none() {
                        "Root (no technician on this session)"
                    } else {
                        "Root (another technician's session)"
                    };
                    ui.label(RichText::new(whose).color(theme::warn(ui)));
                    ui.end_row();
                }
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
            if summoned {
                self.summoned = None;
                self.dismissed.remove(&req.id);
            } else {
                self.snoozed.insert(req.id.clone(), now + SNOOZE_SECS);
            }
            self.deny_note.clear();
            self.answers.clear();
        } else if decision.is_some() && crate::plugins::remote::remote_input_recent() {
            self.last_error = Some(REMOTE_REFUSED.into());
        } else if let Some((status, note, answers)) = decision {
            self.deny_note.clear();
            self.answers.clear();
            self.decide(&req, status, note, answers);
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

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::Datetime;

    fn viewer(key: &str, root: bool) -> ApprovalViewer {
        ApprovalViewer { id: RecordId::new("user", key), root }
    }

    fn row(key: &str, owner: Option<&str>, ttl_secs: i64) -> AgentApproval {
        AgentApproval {
            id: RecordId::new(AGENT_APPROVAL_TABLE, key),
            thread: RecordId::new("agent_thread", "t"),
            kind: "tool_call".into(),
            method: String::new(),
            codex_request_id: String::new(),
            summary: format!("run desktop_click ({key})"),
            server: None,
            tool: Some("desktop_click".into()),
            arguments: None,
            params: None,
            questions: None,
            answers: None,
            response_sent: None,
            status: "pending".into(),
            assignee: owner.map(|k| RecordId::new("user", k)),
            connection_string: Some("PC-1:abc".into()),
            store: Some("MUR".into()),
            requested_at: None,
            expires_at: Datetime::from_timestamp(Datetime::now().timestamp() + ttl_secs, 0),
            decided_at: None,
            sent_to_codex_at: None,
            decided_by: None,
            deny_note: None,
        }
    }

    fn keys(rows: &[AgentApproval]) -> Vec<String> {
        rows.iter().map(|r| r.id.key_string()).collect()
    }

    fn queue_for(v: ApprovalViewer) -> AgentApprovalQueue {
        let mut q = AgentApprovalQueue::default();
        q.set_viewer(Some(v));
        q
    }

    #[test]
    fn root_gets_its_own_rows_in_the_modal_and_others_as_toasts() {
        let rows = vec![row("mine", Some("boss"), 600), row("theirs", Some("tech"), 600), row("nobody", None, 600)];
        let (modal, toast) = split(rows, Some(&viewer("boss", true)));
        assert_eq!(keys(&modal), ["mine"]);
        assert_eq!(keys(&toast), ["theirs", "nobody"]);
    }

    #[test]
    fn a_technician_gets_only_their_own_rows() {
        let rows = vec![row("mine", Some("tech"), 600), row("mates", Some("mate"), 600), row("nobody", None, 600)];
        let (modal, toast) = split(rows, Some(&viewer("tech", false)));
        assert_eq!(keys(&modal), ["mine"]);
        assert!(toast.is_empty());
        let (modal, toast) = split(vec![row("mine", Some("tech"), 600)], None);
        assert!(modal.is_empty() && toast.is_empty());
    }

    #[test]
    fn a_summoned_row_goes_in_front_until_it_lapses() {
        let mut q = queue_for(viewer("boss", true));
        q.apply_snapshot(vec![row("mine", Some("boss"), 600), row("theirs", Some("tech"), 600)], 1);
        assert_eq!(q.front(0.0).map(|(r, s)| (r.id.key_string(), s)), Some(("mine".into(), false)));
        q.summoned = Some(RecordId::new(AGENT_APPROVAL_TABLE, "theirs"));
        assert_eq!(q.front(0.0).map(|(r, s)| (r.id.key_string(), s)), Some(("theirs".into(), true)));
        q.others = vec![row("theirs", Some("tech"), -5)];
        assert_eq!(q.front(0.0).map(|(r, s)| (r.id.key_string(), s)), Some(("mine".into(), false)));
    }

    #[test]
    fn a_hidden_row_is_never_decided() {
        let mut q = queue_for(viewer("mate", false));
        q.decide(&row("theirs", Some("tech"), 600), "accepted", None, None);
        assert!(q.in_flight.is_empty());
        assert_eq!(q.last_error.as_deref(), Some(NOT_PERMITTED));
        let mut nobody = AgentApprovalQueue::default();
        nobody.decide(&row("theirs", Some("tech"), 600), "accepted", None, None);
        assert!(nobody.in_flight.is_empty());
    }

    #[test]
    fn snoozes_survive_a_sign_in_gap_but_not_a_different_user() {
        let mut q = queue_for(viewer("tech", false));
        let id = RecordId::new(AGENT_APPROVAL_TABLE, "mine");
        q.snoozed.insert(id.clone(), 1_000.0);
        q.dismissed.insert(id.clone());
        q.set_viewer(None);
        q.set_viewer(Some(viewer("tech", false)));
        assert!(q.snoozed.contains_key(&id) && q.dismissed.contains(&id));
        q.set_viewer(Some(viewer("mate", false)));
        assert!(q.snoozed.is_empty() && q.dismissed.is_empty());
    }

    #[test]
    fn a_refused_row_returns_only_with_a_snapshot_issued_after_the_click() {
        let mut q = queue_for(viewer("tech", false));
        q.poll_seq = 4;
        q.apply_snapshot(vec![row("mine", Some("tech"), 600)], 4);
        q.remove(&RecordId::new(AGENT_APPROVAL_TABLE, "mine"), false);
        q.apply_snapshot(vec![row("mine", Some("tech"), 600)], 4);
        assert!(q.pending.is_empty(), "a snapshot from before the click stays hidden");
        q.apply_snapshot(vec![row("mine", Some("tech"), 600)], 5);
        assert_eq!(keys(&q.pending), ["mine"], "a reopened row comes back");
    }

    #[test]
    fn a_decided_row_stays_out_of_later_snapshots() {
        let mut q = queue_for(viewer("tech", false));
        q.apply_snapshot(vec![row("mine", Some("tech"), 600)], 1);
        q.remove(&RecordId::new(AGENT_APPROVAL_TABLE, "mine"), true);
        q.apply_snapshot(vec![row("mine", Some("tech"), 600)], 9);
        assert!(q.pending.is_empty());
    }
}

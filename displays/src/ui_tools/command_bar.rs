//! Ctrl+K command bar: one line to the signed-in user's AI session, with the record in view as context.

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::agent_chat::{self, ReplyState};
use database::schema::service_task::find_service_task;
use database::schema::{LiveTaskPayload, RecordId, RecordIdExt, general_connection};
use eframe::egui::{
    Align2, Area, Context, Frame, Id, Key, KeyboardShortcut, Margin, Modifiers, Order, RichText, ScrollArea, TextEdit,
    vec2,
};

use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner, TaskUiActions};

const POLL: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_secs(600);
const SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);

/// A task the bar tells the assistant about.
#[derive(Clone, Debug)]
pub struct FocusedTask {
    pub id: RecordId,
    pub name: String,
    pub service_number: Option<String>,
}

enum Update {
    Progress(String),
    Done(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Waiting,
    Done,
    Failed,
}

pub struct CommandBar {
    open: bool,
    focus: bool,
    input: String,
    last_task: Option<FocusedTask>,
    context: Option<FocusedTask>,
    client: Option<String>,
    phase: Phase,
    reply: String,
    tx: Sender<Update>,
    rx: Receiver<Update>,
}

impl Default for CommandBar {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            open: false,
            focus: false,
            input: String::new(),
            last_task: None,
            context: None,
            client: None,
            phase: Phase::Idle,
            reply: String::new(),
            tx,
            rx,
        }
    }
}

/// Service numbers (seven digits starting with 2) mentioned in `text`, in order, once each.
pub fn service_numbers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(|c: char| !c.is_ascii_digit()) {
        if word.len() == 7 && word.starts_with('2') && !out.iter().any(|s| s == word) {
            out.push(word.to_string());
        }
    }
    out
}

/// The message sent to the assistant: the record in view, then what was typed.
pub fn compose(input: &str, task: Option<&FocusedTask>, client: Option<&str>) -> String {
    let mut lines = Vec::new();
    if let Some(t) = task {
        let sn = t.service_number.as_deref().map(|s| format!(", service {s}")).unwrap_or_default();
        lines.push(format!("[Viewing task \"{}\"{sn}, task id {}]", t.name, t.id.key_string()));
    }
    if let Some(cs) = client {
        lines.push(format!("[Focused client {cs}]"));
    }
    lines.push(input.trim().to_string());
    lines.join("\n")
}

impl CommandBar {
    /// Remembers the task whose modal was opened last.
    pub fn focus_task(&mut self, task: &LiveTaskPayload) {
        self.last_task = Some(FocusedTask {
            id: task.id.clone(),
            name: task.task_name.clone(),
            service_number: task.service_number.clone(),
        });
    }

    fn drain(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::Progress(text) => self.reply = text,
                Update::Done(text) => {
                    self.reply = text;
                    self.phase = Phase::Done;
                }
                Update::Failed(e) => {
                    self.reply = e;
                    self.phase = Phase::Failed;
                }
            }
        }
    }

    fn send(&mut self, email: String, store: Option<String>) {
        let text = compose(&self.input, self.context.as_ref(), self.client.as_deref());
        let service_number = self.context.as_ref().and_then(|t| t.service_number.clone());
        self.phase = Phase::Waiting;
        self.reply.clear();
        let tx = self.tx.clone();
        PlatformSpawner::spawn(async move {
            let cs = general_connection(&email);
            let sent = agent_chat::send(&cs, Some(&email), store.as_deref(), service_number.as_deref(), &text).await;
            let sent = match sent {
                Ok(sent) => sent,
                Err(e) => {
                    let _ = tx.send(Update::Failed(format!("Could not reach the assistant: {e}")));
                    return;
                }
            };
            let mut waited = Duration::ZERO;
            loop {
                match agent_chat::poll_reply(&sent.thread, sent.after_seq).await {
                    Ok(ReplyState::Done(text)) => {
                        let _ = tx.send(Update::Done(text));
                        return;
                    }
                    Ok(ReplyState::Waiting(Some(partial))) => {
                        let _ = tx.send(Update::Progress(partial));
                    }
                    Ok(ReplyState::Waiting(None)) => {}
                    Err(e) => {
                        let _ = tx.send(Update::Failed(e.to_string()));
                        return;
                    }
                }
                if waited >= REPLY_TIMEOUT {
                    let _ = tx.send(Update::Failed("No reply within 10 minutes; see the Ai tab.".into()));
                    return;
                }
                database::sleep_compat(POLL).await;
                waited += POLL;
            }
        });
    }

    /// Toggles on Ctrl+K and draws the bar while open; consumes Escape to close it.
    pub fn ui(
        &mut self,
        ctx: &Context,
        user: Option<&database::schema::User>,
        open_tasks: &[String],
        focused_client: Option<String>,
        ui_actions_tx: Sender<TaskUiActions>,
    ) {
        self.drain();
        let remote = crate::plugins::remote::remote_input_recent();
        if !remote && ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT)) {
            self.open = !self.open;
            if self.open {
                self.focus = true;
                self.context = self.last_task.clone().filter(|t| open_tasks.contains(&t.name));
                self.client = focused_client;
            }
        }
        if !self.open {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.open = false;
            return;
        }
        let Some(user) = user else { return };
        let email = user.get_email().to_string();
        let store = Some(user.get_store().as_str().to_string());

        Area::new(Id::new("command_bar")).anchor(Align2::CENTER_TOP, vec2(0.0, 64.0)).order(Order::Foreground).show(
            ctx,
            |ui| {
                Frame::popup(ui.style()).inner_margin(Margin::same(10)).show(ui, |ui| {
                    ui.set_width(560.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} Ask the assistant", icons::COMMAND_BAR)).strong());
                        ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                            ui.label(RichText::new("Esc closes").small().weak());
                        });
                    });
                    if let Some(t) = &self.context {
                        ui.label(RichText::new(format!("Viewing: {}", t.name)).small().color(theme::weak_text(ui)));
                    }
                    let busy = self.phase == Phase::Waiting;
                    let edit = TextEdit::singleline(&mut self.input)
                        .hint_text("e.g. remind Sam every Monday at 10 to count thermal paste")
                        .desired_width(f32::INFINITY)
                        .interactive(!busy)
                        .show(ui)
                        .response;
                    if self.focus {
                        edit.request_focus();
                        self.focus = false;
                    }
                    let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if submitted && !busy && !self.input.trim().is_empty() {
                        self.send(email.clone(), store.clone());
                        self.input.clear();
                    }
                    match self.phase {
                        Phase::Idle => {}
                        Phase::Waiting => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Working… approvals, if any, pop up separately.").small().weak(),
                                );
                            });
                        }
                        Phase::Failed => {
                            ui.label(RichText::new(&self.reply).small().color(theme::error(ui)));
                        }
                        Phase::Done => {}
                    }
                    if !self.reply.is_empty() && self.phase != Phase::Failed {
                        ui.separator();
                        ScrollArea::vertical().id_salt("command_bar_reply").max_height(320.0).show(ui, |ui| {
                            crate::markdown_editor::chat_markdown::render(ui, &self.reply);
                        });
                        let numbers = service_numbers(&self.reply);
                        if !numbers.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                for sn in numbers.into_iter().take(6) {
                                    if ui.small_button(format!("{} {sn}", icons::OPEN)).clicked() {
                                        let tx = ui_actions_tx.clone();
                                        PlatformSpawner::spawn(async move {
                                            match find_service_task(&sn).await {
                                                Ok(Some(task)) => {
                                                    let _ = tx.try_send(TaskUiActions::OpenTaskModalById(task.id));
                                                }
                                                _ => {
                                                    let _ = crate::get_toast_sender().try_send(
                                                        crate::ToastMessage::Warning(format!(
                                                            "No task found for service {sn}"
                                                        )),
                                                    );
                                                }
                                            }
                                        });
                                    }
                                }
                            });
                        }
                    }
                });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_numbers_are_found_once_each() {
        let text = "Kayleen Reese - 2154905 is waiting; 2154905 again, and 2155144. Phone 801-564-0173, SO 12345678.";
        assert_eq!(service_numbers(text), vec!["2154905", "2155144"]);
    }

    #[test]
    fn the_message_leads_with_the_record_in_view() {
        let task = FocusedTask {
            id: RecordId::new("task", "abc"),
            name: "Kayleen Reese - 2154905".into(),
            service_number: Some("2154905".into()),
        };
        let text = compose("  remind me tomorrow to call her ", Some(&task), None);
        assert_eq!(
            text,
            "[Viewing task \"Kayleen Reese - 2154905\", service 2154905, task id abc]\nremind me tomorrow to call her"
        );
        assert_eq!(compose("hi", None, Some("DESKTOP-1:abc")), "[Focused client DESKTOP-1:abc]\nhi");
    }
}

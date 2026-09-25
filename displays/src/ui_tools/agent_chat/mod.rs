//! Controls shared by the AI chat rail and the Agent Sessions tab: status badges, the context bar,
//! the composer with its queue and attachments, renaming, and the pictures a message carried.

pub mod attach;
mod composer;

pub use composer::{Composer, ComposerAction};

use database::schema::agent_thread::compact_tokens;
use database::schema::{AgentActivity, AgentThread, QueuedTurn, RecordId, RecordIdExt};
use eframe::egui::{self, vec2, Align, Color32, Id, Key, Layout, ProgressBar, Rect, RichText, Spinner, TextEdit, Ui};

use crate::ui_tools::chat_bubble::{self, ChatKind, ChatRow, ChatStyle};
use crate::ui_tools::{icons, theme};

/// Share of the context window above which Compact is emphasised.
pub const COMPACT_EMPHASIS: f32 = 0.7;
/// Characters of a queued message shown in the queue strip.
const QUEUED_PREVIEW_CHARS: usize = 120;
/// Side of a sent picture's thumbnail in the transcript.
const SENT_THUMB: f32 = 96.0;
/// Width of an enlarged thumbnail in a hover tooltip.
const PREVIEW_WIDTH: f32 = 320.0;

/// Icon, colour and word for a thread status.
pub fn status_chip(ui: &Ui, status: &str) -> (&'static str, Color32, &'static str) {
    match status {
        "queued" => (icons::STATUS_QUEUED, theme::weak_text(ui), "Queued"),
        "starting" => (icons::STATUS_WAIT, theme::info(ui), "Starting"),
        "idle" => (icons::STATUS_READY, theme::success(ui), "Idle"),
        "running" => (icons::STATUS_ON, theme::info(ui), "Working"),
        "waiting_approval" => (icons::LOCK, theme::warn(ui), "Needs approval"),
        "closed" => (icons::STATUS_OFF, theme::weak_text(ui), "Closed"),
        "failed" => (icons::STATUS_ERR, theme::error(ui), "Failed"),
        _ => (icons::STATUS_DOT, theme::weak_text(ui), "Unknown"),
    }
}

/// True while the thread's agent is starting or working a turn.
pub fn is_active(thread: &AgentThread) -> bool {
    matches!(thread.status.as_str(), "starting" | "running")
}

/// Words for a thread's status: the current activity while a turn runs.
pub fn status_words(thread: &AgentThread) -> String {
    match thread.status.as_str() {
        "running" | "waiting_approval" => thread.activity().label(),
        other => status_word(other).to_string(),
    }
}

fn status_word(status: &str) -> &'static str {
    match status {
        "queued" => "Queued",
        "starting" => "Starting",
        "idle" => "Idle",
        "closed" => "Closed",
        "failed" => "Failed",
        _ => "Unknown",
    }
}

/// A thread's status as a spinner or icon followed by its words.
pub fn status_badge(ui: &mut Ui, thread: &AgentThread) {
    let (icon, color, _) = status_chip(ui, &thread.status);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if is_active(thread) {
            ui.add(Spinner::new().size(12.0).color(color));
        } else {
            ui.label(RichText::new(icon).color(color).small());
        }
        ui.label(RichText::new(status_words(thread)).color(color).small());
    });
}

/// Bar colour for a share of the context window: green, then amber, then red.
pub fn context_color(ui: &Ui, fraction: f32) -> Color32 {
    let (ok, warn, bad) = (theme::success(ui), theme::warn(ui), theme::error(ui));
    match fraction {
        f if f < 0.5 => ok,
        f if f < COMPACT_EMPHASIS => lerp(ok, warn, (f - 0.5) / (COMPACT_EMPHASIS - 0.5)),
        f if f < 0.9 => lerp(warn, bad, (f - COMPACT_EMPHASIS) / (0.9 - COMPACT_EMPHASIS)),
        _ => bad,
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

/// Context use as a bar with its percent and token counts, and a Compact button; true when Compact was pressed.
pub fn context_bar(ui: &mut Ui, thread: &AgentThread) -> bool {
    let mut compact = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let fraction = thread.context_fraction();
        if let (Some(f), Some((used, window))) = (fraction, thread.context_tokens()) {
            let color = context_color(ui, f);
            ui.add(ProgressBar::new(f.clamp(0.0, 1.0)).desired_width(96.0).desired_height(8.0).fill(color))
                .on_hover_text(format!("{used} of {window} tokens in the context window"));
            ui.label(
                RichText::new(format!("{:.0}% \u{00b7} {}/{}", f * 100.0, compact_tokens(used), compact_tokens(window)))
                    .small()
                    .color(color),
            );
        } else {
            ui.label(RichText::new("context \u{2014}").small().weak());
        }
        if thread.activity() == AgentActivity::Compacting {
            ui.add(Spinner::new().size(12.0));
            ui.label(RichText::new("Compacting\u{2026}").small().weak());
            return;
        }
        let emphasised = fraction.is_some_and(|f| f >= COMPACT_EMPHASIS);
        let label = format!("{} Compact", icons::COMPACT);
        let button = if emphasised {
            egui::Button::new(RichText::new(label).small().strong().color(theme::strong_text(ui))).fill(context_color(ui, fraction.unwrap_or(1.0)).gamma_multiply(0.45))
        } else {
            egui::Button::new(RichText::new(label).small())
        };
        let tip = if thread.is_busy() {
            "Summarise the conversation to free context once this turn ends"
        } else {
            "Summarise the conversation to free context"
        };
        compact = ui.add_enabled(thread.is_open(), button).on_hover_text(tip).clicked();
    });
    compact
}

/// What the technician did with the queue strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    Remove(RecordId),
    Edit(RecordId),
    Resume,
}

/// The thread's waiting messages, each with a way back into the composer or out, and Resume while held.
pub fn queue_strip(ui: &mut Ui, waiting: &[QueuedTurn]) -> Option<QueueAction> {
    if waiting.is_empty() {
        return None;
    }
    let mut action = None;
    let held = waiting.iter().any(QueuedTurn::is_held);
    ui.horizontal_wrapped(|ui| {
        if held {
            ui.label(RichText::new(format!("{} Queue held after a stop or a failed turn", icons::PAUSE)).small().color(theme::warn(ui)));
            if ui.small_button(format!("{} Resume", icons::PLAY)).on_hover_text("Send the queued messages, one per turn").clicked() {
                action = Some(QueueAction::Resume);
            }
        } else {
            ui.label(RichText::new(format!("{} {} queued \u{00b7} sent when the turn ends", icons::QUEUE, waiting.len())).small().color(theme::info(ui)));
        }
    });
    for turn in waiting {
        ui.push_id(turn.id.key_string(), |ui| {
            ui.horizontal(|ui| {
                let buttons = 2.0 * (24.0 + ui.spacing().item_spacing.x);
                let first = turn.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
                let mut shown = chat_bubble::clip(first, QUEUED_PREVIEW_CHARS).into_owned();
                if !turn.image_names.is_empty() {
                    shown = format!("{} {} {shown}", icons::IMAGE, turn.image_names.len());
                }
                ui.allocate_ui_with_layout(vec2((ui.available_width() - buttons).max(40.0), 20.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(shown).small()).truncate()).on_hover_text(&turn.text);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button(icons::CLOSE).on_hover_text("Remove from the queue").clicked() {
                        action = Some(QueueAction::Remove(turn.id.clone()));
                    }
                    if ui.small_button(icons::UNDO).on_hover_text("Back to the composer").clicked() {
                        action = Some(QueueAction::Edit(turn.id.clone()));
                    }
                });
            });
        });
    }
    action
}

/// A chat title being edited in place, keyed by the chat it names.
#[derive(Debug, Clone)]
pub struct Rename {
    pub key: String,
    pub text: String,
    focused: bool,
}

/// What a frame of editing ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameOutcome {
    Editing,
    Save(String, String),
    Cancel,
}

impl Rename {
    pub fn new(key: impl Into<String>, current: &str) -> Self {
        Self { key: key.into(), text: current.to_string(), focused: false }
    }

    /// Draws the field; Enter saves, Escape or clicking away cancels.
    pub fn show(&mut self, ui: &mut Ui, width: f32) -> RenameOutcome {
        let id = Id::new(("agent_chat_rename", &self.key));
        let response = ui.add(TextEdit::singleline(&mut self.text).id(id).desired_width(width).hint_text("New title"));
        if !self.focused {
            response.request_focus();
            self.focused = true;
            return RenameOutcome::Editing;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return RenameOutcome::Cancel;
        }
        if response.lost_focus() {
            return match database::schema::clean_title(&self.text) {
                Some(title) if ui.input(|i| i.key_pressed(Key::Enter)) => RenameOutcome::Save(self.key.clone(), title),
                _ => RenameOutcome::Cancel,
            };
        }
        RenameOutcome::Editing
    }
}

/// A sent message: the typed text, each inlined text file as a collapsed row, then its pictures.
pub fn user_body(ui: &mut Ui, style: &ChatStyle, text: &str, images: &[String], id: Id) {
    let (typed, files) = attach::split_inlined(text);
    if !typed.trim().is_empty() {
        chat_bubble::markdown(ui, style, typed, style.text, id);
    }
    for (n, file) in files.iter().enumerate() {
        let key = format!("file{n}");
        let size = format!("{} KB", file.body.len().div_ceil(1024));
        ChatRow::new(ChatKind::FileChange, &key, file.name)
            .nested(true)
            .badge(size, style.weak)
            .copy(file.body)
            .show(ui, style, id, |ui, sub| chat_bubble::code(ui, style, file.lang, file.body, sub.with("code")));
    }
    if !images.is_empty() {
        sent_images(ui, images);
    }
}

/// Thumbnails of the pictures a sent message carried, or their names where this app kept no thumbnail.
pub fn sent_images(ui: &mut Ui, names: &[String]) {
    ui.horizontal_wrapped(|ui| {
        for name in names {
            match attach::sent_thumb(name) {
                Some(tex) => {
                    let image = egui::Image::from_texture(&tex).max_size(vec2(SENT_THUMB, SENT_THUMB)).corner_radius(4.0);
                    ui.add(image).on_hover_ui(|ui| {
                        ui.add(egui::Image::from_texture(&tex).max_width(PREVIEW_WIDTH));
                        ui.label(RichText::new(name).small().weak());
                    });
                }
                None => {
                    ui.label(RichText::new(format!("{} {name}", icons::IMAGE)).small().weak());
                }
            }
        }
    });
}

/// Paints a drop hint over `rect` while files are dragged over the window.
pub fn drop_hint(ui: &Ui, rect: Rect) {
    if ui.ctx().input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new(("agent_chat_drop", rect.min.x as i32, rect.min.y as i32))));
    painter.rect_filled(rect, 6.0, Color32::from_black_alpha(150));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("{} Drop to attach", icons::PAPERCLIP),
        egui::FontId::proportional(18.0),
        theme::strong_text(ui),
    );
}

/// True when the pointer was last seen over `rect`.
pub fn hovered(ui: &Ui, rect: Rect) -> bool {
    ui.ctx().input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, RawInput};

    #[test]
    fn the_context_colour_ramps_from_green_through_amber_to_red() {
        let ctx = Context::default();
        let mut seen = Vec::new();
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            seen = [0.1, 0.49, 0.6, 0.7, 0.8, 0.95].iter().map(|f| context_color(ui, *f)).collect();
            assert_eq!(seen[0], theme::success(ui));
            assert_eq!(seen[3], theme::warn(ui));
            assert_eq!(seen[5], theme::error(ui));
        });
        out.textures_delta.clear();
        assert_ne!(seen[2], seen[1], "the ramp moves between the bands");
        assert_ne!(seen[4], seen[3]);
    }

    #[test]
    fn status_words_read_the_activity_while_a_turn_runs() {
        let mut t = AgentThread {
            id: RecordId::new("agent_thread", "t"),
            status: "running".into(),
            connection_string: String::new(),
            hostname: None,
            service_number: None,
            store: None,
            requested_by: None,
            assignee: None,
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
            tokens_used: None,
            tokens_window: None,
            last_seq: None,
            activity: Some("tool:get_client_info".into()),
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        };
        assert_eq!(status_words(&t), "Running get_client_info");
        assert!(is_active(&t));
        t.status = "waiting_approval".into();
        t.activity = Some("approval:remote_exec_start".into());
        assert_eq!(status_words(&t), "Needs approval: remote_exec_start");
        assert!(!is_active(&t));
        t.status = "idle".into();
        assert_eq!(status_words(&t), "Idle");
    }
}

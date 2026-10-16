//! Agent chat controls shared by the AI chat rail and the Agent Sessions tab.

pub mod attach;
mod composer;

pub use composer::{Composer, ComposerAction};

use std::fmt::Display;

use chrono::{DateTime, TimeZone, Utc};
use database::schema::agent_thread::compact_tokens;
use database::schema::{AgentActivity, AgentThread, QueuedTurn, RecordId, RecordIdExt};
use eframe::egui::{self, Align, Color32, Id, Key, Layout, Rect, RichText, Ui, Vec2, vec2};

use crate::ui_tools::chat_bubble::{self, ChatKind, ChatRow, ChatStyle};
use crate::ui_tools::controls::{
    self, ButtonKind, IconBox, IconButton, IconLabel, Metrics, SpinnerSize, TextButton, TextField,
    Tokens,
};
use crate::ui_tools::icons::svg::{self, Icon};
use crate::ui_tools::{fonts, icons, theme};

/// Share of the context window above which Compact is emphasised.
pub const COMPACT_EMPHASIS: f32 = 0.7;
/// Characters of a queued message shown in the queue strip.
const QUEUED_PREVIEW_CHARS: usize = 120;
/// Side of a sent picture's thumbnail in the transcript.
const SENT_THUMB: f32 = 96.0;
/// Width of an enlarged thumbnail in a hover tooltip.
const PREVIEW_WIDTH: f32 = 320.0;
/// Width of the context bar.
const CONTEXT_BAR_W: f32 = 96.0;
/// Opacity of the panel colour behind the drop hint.
const DROP_SCRIM: f32 = 0.6;
/// Size of the drop hint's icon and text.
const DROP_ICON: f32 = 32.0;
const DROP_TEXT_PT: f32 = 16.0;

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

/// The SVG icon for a thread status, matching [`status_chip`].
pub fn status_icon(status: &str) -> Icon {
    match status {
        "queued" => svg::QUEUE,
        "starting" => svg::HOURGLASS,
        "idle" => svg::CHECK_CIRCLE,
        "running" => svg::PLAY,
        "waiting_approval" => svg::LOCK,
        "closed" => svg::MINUS_CIRCLE,
        "failed" => svg::X_CIRCLE,
        _ => svg::DOT,
    }
}

/// An icon then small text, both in `color`.
fn icon_text(ui: &mut Ui, icon: Icon, text: &str, color: Color32) -> egui::Response {
    IconLabel::new(icon, text).tone(color).small(true).show(ui)
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

/// `fleet_route` of a request that compacted the context.
const COMPACTION_ROUTE: &str = "compaction";

/// A trimmed, non-empty `fleet_*` value.
fn stamped(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// The node that served the thread's latest request and its quant, joined by `sep`; `None` while no node is stamped.
pub fn fleet_node_quant(thread: &AgentThread, sep: &str) -> Option<String> {
    let node = stamped(thread.fleet_node.as_deref())?;
    Some(match stamped(thread.fleet_quant.as_deref()) {
        Some(quant) => format!("{node}{sep}{quant}"),
        None => node.to_string(),
    })
}

/// [`fleet_node_quant`], then ` · compaction` when that request compacted the context.
pub fn fleet_label(thread: &AgentThread, sep: &str) -> Option<String> {
    let label = fleet_node_quant(thread, sep)?;
    let compaction = stamped(thread.fleet_route.as_deref()) == Some(COMPACTION_ROUTE);
    Some(if compaction {
        format!("{label} \u{00b7} {COMPACTION_ROUTE}")
    } else {
        label
    })
}

/// `just now`, `2 min ago`, `3 h ago` or `4 d ago`.
fn ago_label(secs: i64) -> String {
    match secs.max(0) {
        0..60 => "just now".to_string(),
        s @ 60..3_600 => format!("{} min ago", s / 60),
        s @ 3_600..86_400 => format!("{} h ago", s / 3_600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// Model file, route and the time of the thread's latest request, one per line, in `now`'s time zone.
pub fn fleet_hover<Tz: TimeZone>(thread: &AgentThread, now: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    let mut lines = vec![match stamped(thread.fleet_model.as_deref()) {
        Some(model) => format!("Model: {model}"),
        None => "Model unknown".to_string(),
    }];
    if let Some(route) = stamped(thread.fleet_route.as_deref()) {
        lines.push(format!("Route: {route}"));
    }
    if let Some(at) = thread.fleet_served_at {
        let at = DateTime::<Utc>::from(at);
        let age = (now.with_timezone(&Utc) - at).num_seconds();
        let clock = chat_bubble::clock_label(&at.with_timezone(&now.timezone()), now);
        lines.push(format!("Last request {clock} ({})", ago_label(age)));
    }
    lines.join("\n")
}

/// A thread's status as a spinner or icon followed by its words.
pub fn status_badge(ui: &mut Ui, thread: &AgentThread) {
    let (_, color, _) = status_chip(ui, &thread.status);
    if !is_active(thread) {
        icon_text(
            ui,
            status_icon(&thread.status),
            &status_words(thread),
            color,
        );
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = Metrics::get(ui.ctx()).gap.x / 2.0;
        controls::spinner(ui, SpinnerSize::Sm);
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
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Context use as a bar with its percent and token counts, and a Compact button when `compactable`; true when Compact was pressed.
pub fn context_bar(ui: &mut Ui, thread: &AgentThread, compactable: bool) -> bool {
    let mut compact = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = Metrics::get(ui.ctx()).gap.x * 0.75;
        let fraction = thread.context_fraction();
        if let (Some(f), Some((used, window))) = (fraction, thread.context_tokens()) {
            let color = context_color(ui, f);
            controls::meter(ui, f, CONTEXT_BAR_W, color)
                .on_hover_text(format!("{used} of {window} tokens in the context window"));
            ui.label(
                RichText::new(format!(
                    "{:.0}% \u{00b7} {}/{}",
                    f * 100.0,
                    compact_tokens(used),
                    compact_tokens(window)
                ))
                .small()
                .color(color),
            );
        } else {
            ui.label(RichText::new("context \u{2014}").small().weak());
        }
        if thread.activity() == AgentActivity::Compacting {
            controls::spinner(ui, SpinnerSize::Sm);
            ui.label(RichText::new("Compacting\u{2026}").small().weak());
            return;
        }
        if !compactable {
            return;
        }
        let emphasised = fraction.is_some_and(|f| f >= COMPACT_EMPHASIS);
        let kind = if emphasised {
            ButtonKind::Primary
        } else {
            ButtonKind::Neutral
        };
        let tip = if thread.is_busy() {
            "Summarise the conversation to free context once this turn ends"
        } else {
            "Summarise the conversation to free context"
        };
        compact = TextButton::new("Compact")
            .icon(svg::ARROWS_IN_LINE_VERTICAL)
            .kind(kind)
            .enabled(thread.is_open())
            .show(ui)
            .on_hover_text(tip)
            .clicked();
    });
    compact
}

/// A warning while every tool call runs without asking, with an Ask again button when `steerable`; true when pressed.
pub fn approve_all_chip(ui: &mut Ui, thread: &AgentThread, steerable: bool) -> bool {
    if !thread.is_open() || !thread.approves_all() {
        return false;
    }
    let mut ask_again = false;
    ui.horizontal_wrapped(|ui| {
        icon_text(
            ui,
            svg::LOCK_OPEN,
            "Approving all tool calls",
            theme::warn(ui),
        )
        .on_hover_text("Every tool call the agent makes on this machine runs without asking");
        ask_again = steerable
            && controls::icon_text_button(ui, svg::LOCK, "Ask again")
                .on_hover_text("Ask the technician before each gated tool call again")
                .clicked();
    });
    ask_again
}

/// What the technician did with the queue strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    Remove(RecordId),
    Edit(RecordId),
    SendNow(RecordId),
    Resume,
}

/// The thread's waiting messages, each with a way back into the composer or out, and Resume while held.
pub fn queue_strip(ui: &mut Ui, waiting: &[QueuedTurn]) -> Option<QueueAction> {
    if waiting.is_empty() {
        return None;
    }
    let mut action = None;
    let held = waiting.iter().any(QueuedTurn::is_held);
    let m = Metrics::get(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        if held {
            icon_text(
                ui,
                svg::PAUSE,
                "Queue held after a stop or a failed turn",
                theme::warn(ui),
            );
            if controls::icon_text_button(ui, svg::PLAY, "Resume")
                .on_hover_text("Send the queued messages, one per turn")
                .clicked()
            {
                action = Some(QueueAction::Resume);
            }
        } else {
            let text = format!("{} queued \u{00b7} sent when the turn ends", waiting.len());
            icon_text(ui, svg::QUEUE, &text, theme::info(ui));
        }
    });
    let side = IconBox::Sm.side(&m);
    for turn in waiting {
        ui.push_id(turn.id.key_string(), |ui| {
            ui.horizontal(|ui| {
                let buttons = 3.0 * (side + ui.spacing().item_spacing.x);
                let first = turn
                    .text
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or_default();
                let mut shown = chat_bubble::clip(first, QUEUED_PREVIEW_CHARS).into_owned();
                if !turn.image_names.is_empty() {
                    shown = format!("{} {} {shown}", icons::IMAGE, turn.image_names.len());
                }
                ui.allocate_ui_with_layout(
                    vec2((ui.available_width() - buttons).max(40.0), side),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(egui::Label::new(RichText::new(shown).small()).truncate())
                            .on_hover_text(&turn.text);
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let tool = |ui: &mut Ui, icon: Icon, tip: &str| {
                        IconButton::new(icon, IconBox::Sm)
                            .tip(tip)
                            .show(ui)
                            .clicked()
                    };
                    if tool(ui, svg::X, "Remove from the queue") {
                        action = Some(QueueAction::Remove(turn.id.clone()));
                    }
                    if tool(ui, svg::ARROW_U_UP_LEFT, "Back to the composer") {
                        action = Some(QueueAction::Edit(turn.id.clone()));
                    }
                    if tool(
                        ui,
                        svg::LIGHTNING,
                        "Send now: the agent reads it after its current step",
                    ) {
                        action = Some(QueueAction::SendNow(turn.id.clone()));
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
        Self {
            key: key.into(),
            text: current.to_string(),
            focused: false,
        }
    }

    /// Draws the field; Enter saves, Escape or clicking away cancels.
    pub fn show(&mut self, ui: &mut Ui, width: f32) -> RenameOutcome {
        let response = TextField::new(&mut self.text)
            .id_salt(("agent_chat_rename", &self.key))
            .hint("New title")
            .icon(svg::PENCIL_SIMPLE)
            .width(width)
            .show(ui);
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
                Some(title) if ui.input(|i| i.key_pressed(Key::Enter)) => {
                    RenameOutcome::Save(self.key.clone(), title)
                }
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
            .show(ui, style, id, |ui, sub| {
                chat_bubble::code(ui, style, file.lang, file.body, sub.with("code"))
            });
    }
    if !images.is_empty() {
        sent_images(ui, images);
    }
}

/// Thumbnails of the pictures a sent message carried, or their names where this app kept no thumbnail.
pub fn sent_images(ui: &mut Ui, names: &[String]) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        for name in names {
            match attach::sent_thumb(name) {
                Some(tex) => {
                    let image = egui::Image::from_texture(&tex)
                        .max_size(vec2(SENT_THUMB, SENT_THUMB))
                        .corner_radius(t.radius_sm);
                    ui.add(image).on_hover_ui(|ui| {
                        ui.add(egui::Image::from_texture(&tex).max_width(PREVIEW_WIDTH));
                        ui.label(RichText::new(name).small().weak());
                    });
                }
                None => {
                    icon_text(ui, svg::IMAGE, name, t.text_dim);
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
    let id = Id::unique(("agent_chat_drop", rect.min.x as i32, rect.min.y as i32));
    let layer = egui::LayerId::new(egui::Order::Foreground, id);
    let over = Ui::new(
        ui.ctx().clone(),
        id,
        egui::UiBuilder::new().layer_id(layer).max_rect(rect),
    );
    let t = Tokens::get(ui.ctx());
    let painter = over.painter();
    painter.rect_filled(rect, t.radius_md, t.base.gamma_multiply(DROP_SCRIM));
    let icon_rect = Rect::from_center_size(
        rect.center() - vec2(0.0, DROP_ICON / 2.0),
        Vec2::splat(DROP_ICON),
    );
    svg::PAPERCLIP.paint(&over, icon_rect, DROP_ICON, t.primary);
    painter.text(
        rect.center() + vec2(0.0, DROP_ICON / 4.0),
        egui::Align2::CENTER_TOP,
        "Drop to attach",
        fonts::semibold(DROP_TEXT_PT),
        t.text,
    );
}

/// True when the pointer was last seen over `rect`.
pub fn hovered(ui: &Ui, rect: Rect) -> bool {
    ui.ctx()
        .input(|i| i.pointer.latest_pos())
        .is_some_and(|p| rect.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, Event, Modifiers, PointerButton, RawInput, pos2};

    /// Every action the strip answers to a click anywhere in its top 80 points, right to left.
    fn strip_actions(waiting: &[QueuedTurn]) -> Vec<QueueAction> {
        let ctx = Context::default();
        let run = |events: Vec<Event>| {
            let input = RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 200.0))),
                ..Default::default()
            };
            let mut action = None;
            let mut out = ctx.run_ui(input, |ui| action = queue_strip(ui, waiting));
            out.textures_delta.clear();
            action
        };
        let mut found: Vec<QueueAction> = Vec::new();
        for y in (2..80).step_by(4) {
            for x in (2..420).rev().step_by(4) {
                let pos = pos2(x as f32, y as f32);
                let button = |pressed| Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                };
                let action =
                    run(vec![Event::PointerMoved(pos), button(true)]).or(run(vec![button(false)]));
                if let Some(action) = action.filter(|a| !found.contains(a)) {
                    found.push(action);
                }
            }
        }
        found
    }

    #[test]
    fn each_queued_message_can_be_removed_taken_back_or_sent_now() {
        let id = RecordId::new("agent_turn", "q1");
        let waiting = vec![QueuedTurn {
            id: id.clone(),
            text: "i plugged in the customer's two drives as well".into(),
            status: "queued".into(),
            tech: None,
            image_names: Vec::new(),
            created_at: None,
        }];
        assert_eq!(
            strip_actions(&waiting),
            vec![
                QueueAction::Remove(id.clone()),
                QueueAction::Edit(id.clone()),
                QueueAction::SendNow(id)
            ]
        );
    }

    #[test]
    fn the_context_colour_ramps_from_green_through_amber_to_red() {
        let ctx = Context::default();
        let mut seen = Vec::new();
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            seen = [0.1, 0.49, 0.6, 0.7, 0.8, 0.95]
                .iter()
                .map(|f| context_color(ui, *f))
                .collect();
            assert_eq!(seen[0], theme::success(ui));
            assert_eq!(seen[3], theme::warn(ui));
            assert_eq!(seen[5], theme::error(ui));
        });
        out.textures_delta.clear();
        assert_ne!(seen[2], seen[1], "the ramp moves between the bands");
        assert_ne!(seen[4], seen[3]);
    }

    fn thread(status: &str) -> AgentThread {
        AgentThread {
            id: RecordId::new("agent_thread", "t"),
            status: status.into(),
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
            tools_hash: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            fleet_node: None,
            fleet_route: None,
            fleet_quant: None,
            fleet_model: None,
            fleet_served_at: None,
            allow_box_shell: false,
            approve_all: None,
            tokens_used: None,
            tokens_window: None,
            last_seq: None,
            activity: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    #[test]
    fn status_words_read_the_activity_while_a_turn_runs() {
        let mut t = thread("running");
        t.activity = Some("tool:get_client_info".into());
        assert_eq!(status_words(&t), "Running get_client_info");
        assert!(is_active(&t));
        t.status = "waiting_approval".into();
        t.activity = Some("approval:remote_exec_start".into());
        assert_eq!(status_words(&t), "Needs approval: remote_exec_start");
        assert!(!is_active(&t));
        t.status = "idle".into();
        assert_eq!(status_words(&t), "Idle");
    }

    fn stamped_thread(node: Option<&str>, quant: Option<&str>, route: Option<&str>) -> AgentThread {
        let mut t = thread("running");
        t.fleet_node = node.map(str::to_string);
        t.fleet_quant = quant.map(str::to_string);
        t.fleet_route = route.map(str::to_string);
        t
    }

    #[test]
    fn the_fleet_label_names_the_node_its_quant_and_a_compaction() {
        let dot = " \u{00b7} ";
        let pinned = stamped_thread(Some("node-7900"), Some("Q8_0"), Some("pinned"));
        assert_eq!(
            fleet_label(&pinned, dot).as_deref(),
            Some("node-7900 \u{00b7} Q8_0")
        );
        assert_eq!(fleet_label(&pinned, " ").as_deref(), Some("node-7900 Q8_0"));
        let compaction = stamped_thread(Some("r9700"), Some("MXFP4 AWQ"), Some("compaction"));
        assert_eq!(
            fleet_label(&compaction, dot).as_deref(),
            Some("r9700 \u{00b7} MXFP4 AWQ \u{00b7} compaction")
        );
        assert_eq!(
            fleet_label(&compaction, " ").as_deref(),
            Some("r9700 MXFP4 AWQ \u{00b7} compaction")
        );
        assert_eq!(
            fleet_node_quant(&compaction, dot).as_deref(),
            Some("r9700 \u{00b7} MXFP4 AWQ")
        );
        let unprobed = stamped_thread(Some("node-128"), None, Some("spill"));
        assert_eq!(fleet_label(&unprobed, dot).as_deref(), Some("node-128"));
        let blank_quant = stamped_thread(Some("node-128"), Some(" "), None);
        assert_eq!(fleet_label(&blank_quant, dot).as_deref(), Some("node-128"));
        assert_eq!(
            fleet_label(&stamped_thread(None, Some("Q8_0"), Some("pinned")), dot),
            None
        );
        assert_eq!(
            fleet_label(&stamped_thread(Some("  "), Some("Q8_0"), None), dot),
            None
        );
    }

    #[test]
    fn ages_read_in_minutes_hours_and_days() {
        assert_eq!(ago_label(-5), "just now");
        assert_eq!(ago_label(59), "just now");
        assert_eq!(ago_label(120), "2 min ago");
        assert_eq!(ago_label(3_599), "59 min ago");
        assert_eq!(ago_label(7_200), "2 h ago");
        assert_eq!(ago_label(3 * 86_400), "3 d ago");
    }

    #[test]
    fn the_fleet_hover_gives_the_model_route_and_last_request() {
        let now = Utc
            .with_ymd_and_hms(2026, 10, 14, 12, 35, 10)
            .single()
            .expect("valid time");
        let mut t = stamped_thread(Some("node-7900"), Some("Q8_0"), Some("pinned"));
        t.fleet_model = Some("Qwen3.8-27B-Q8_0.gguf".into());
        t.fleet_served_at = Some((now - chrono::TimeDelta::seconds(150)).into());
        assert_eq!(
            fleet_hover(&t, &now),
            "Model: Qwen3.8-27B-Q8_0.gguf\nRoute: pinned\nLast request 12:32 (2 min ago)"
        );
        let bare = stamped_thread(Some("node-128"), None, None);
        assert_eq!(fleet_hover(&bare, &now), "Model unknown");
    }
}

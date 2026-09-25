//! The message box both chat surfaces share: attachments, the text, and Send, Queue, Send now and Stop.

use base64::Engine;
use crossbeam::channel::{Receiver, Sender};
use database::schema::{AgentTurn, TurnImage};
use eframe::egui::{
    self, Align, Button, Id, Key, KeyboardShortcut, Layout, Modifiers, Rect, RichText, ScrollArea,
    Spinner, TextEdit, Ui, vec2,
};

use super::attach::{self, AttachEvent, Attachment, Body};
use crate::ToastMessage;
use crate::ui_tools::{icons, theme};

/// What the composer asked for this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAction {
    /// A message, its pictures and their staged file names, as a `start`, `queue` or `steer` turn.
    Send {
        kind: &'static str,
        text: String,
        images: Vec<TurnImage>,
        staged: Vec<String>,
    },
    Stop,
}

/// Memory key naming the composer the technician last used.
const ACTIVE_KEY: &str = "agent_chat_active_composer";
/// Seconds after a text paste in which a Ctrl+V release reads no clipboard picture.
#[cfg(not(target_arch = "wasm32"))]
const TEXT_PASTE_GRACE: f64 = 1.0;
/// Side of a composer thumbnail.
const THUMB: f32 = 56.0;
/// Width of each button column beside the text box.
const SIDE_BUTTON_W: f32 = 30.0;

/// Attachments waiting to go out with the next message, and the reads still under way.
pub struct Composer {
    pub attachments: Vec<Attachment>,
    tx: Sender<AttachEvent>,
    rx: Receiver<AttachEvent>,
    reading: usize,
    text_pasted_at: Option<f64>,
}

impl Default for Composer {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            attachments: Vec::new(),
            tx,
            rx,
            reading: 0,
            text_pasted_at: None,
        }
    }
}

fn toast(text: String) {
    let _ = crate::get_toast_sender().try_send(ToastMessage::Warning(text));
}

impl Composer {
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                AttachEvent::Started => self.reading += 1,
                AttachEvent::Ready(prepared) => {
                    self.reading = self.reading.saturating_sub(1);
                    let pictures = self
                        .attachments
                        .iter()
                        .filter(|a| matches!(a.body, Body::Image { .. }))
                        .count();
                    if matches!(prepared.body, Body::Image { .. }) && pictures >= attach::MAX_IMAGES
                    {
                        toast(format!(
                            "A message carries at most {} pictures; {} was left out.",
                            attach::MAX_IMAGES,
                            prepared.name
                        ));
                        continue;
                    }
                    self.attachments.push(prepared.into_attachment(ctx));
                }
                AttachEvent::Refused(why) => {
                    self.reading = self.reading.saturating_sub(1);
                    if !why.is_empty() {
                        toast(why);
                    }
                }
            }
        }
    }

    /// Draws the attachments, then the text box with its button columns; while `busy` it offers Queue, Send now and Stop.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        id: Id,
        text: &mut String,
        busy: bool,
        enabled: bool,
        max_text_height: f32,
    ) -> Option<ComposerAction> {
        let ctx = ui.ctx().clone();
        self.receive(&ctx);
        #[cfg(target_arch = "wasm32")]
        attach::web_paste::install(&ctx);
        let text_id = id.with("text");
        let focused = ui.memory(|m| m.has_focus(text_id));
        let enter = enabled && focused && ui.input_mut(take_plain_enter);
        let mut action = None;
        ui.add_enabled_ui(enabled, |ui| {
            self.attachment_strip(ui);
            let hint = if busy {
                "The agent is working: Enter queues this for when it finishes"
            } else {
                "Message the agent (Shift+Enter for a new line; paste or drop pictures and files)"
            };
            let button_h = (ui.text_style_height(&egui::TextStyle::Button)
                + 2.0 * ui.spacing().button_padding.y)
                .max(ui.spacing().interact_size.y);
            let column_h = 2.0 * button_h + ui.spacing().item_spacing.y;
            let max_text_height = max_text_height.max(column_h);
            // Vertical text padding that makes two rows as tall as two stacked buttons.
            let font = egui::FontSelection::default().resolve(ui.style());
            let line_h =
                ui.fonts_mut(|f| f.row_height(&font)) + ui.spacing().extra_text_line_spacing;
            let pad_y = ((column_h - 2.0 * line_h) / 2.0).ceil().clamp(2.0, 40.0) as i8;
            ui.horizontal_top(|ui| {
                let gap = ui.spacing().item_spacing.x;
                let columns = if busy { 2.0 } else { 1.0 };
                let text_w =
                    (ui.available_width() - columns * (SIDE_BUTTON_W + gap)).max(SIDE_BUTTON_W);
                let edit = ui.allocate_ui_with_layout(
                    vec2(text_w, max_text_height),
                    Layout::top_down_justified(Align::Min),
                    |ui| {
                        ScrollArea::vertical()
                            .id_salt(id.with("scroll"))
                            .max_height(max_text_height)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                TextEdit::multiline(text)
                                    .id(text_id)
                                    .hint_text(hint)
                                    .return_key(Some(KeyboardShortcut::new(
                                        Modifiers::SHIFT,
                                        Key::Enter,
                                    )))
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .margin(egui::Margin::symmetric(4, pad_y))
                                    .show(ui)
                                    .response
                            })
                            .inner
                    },
                );
                let response = edit.inner;
                if response.has_focus() {
                    mark_active(ui, id);
                }
                self.watch_paste(ui, response.has_focus());
                action = self.controls(ui, text, busy, edit.response.rect.height());
            });
        });
        if enter {
            action = self
                .submit(text, if busy { "queue" } else { "start" })
                .or(action);
            ui.memory_mut(|m| m.request_focus(text_id));
        }
        action
    }

    /// Paints a drop hint over `rect` and takes dropped files when this is the chat the technician last used.
    pub fn drop_zone(&mut self, ui: &Ui, id: Id, rect: Rect) {
        if super::hovered(ui, rect) {
            mark_active(ui, id);
        }
        let active = ui.data(|d| d.get_temp::<Id>(Id::new(ACTIVE_KEY))) == Some(id);
        if !active {
            return;
        }
        super::drop_hint(ui, rect);
        if ui.ctx().input(|i| !i.raw.dropped_files.is_empty()) {
            attach::take_dropped(ui.ctx(), &self.tx);
        }
    }

    /// Puts a queued message back in the box: its text, its inlined files and its pictures.
    pub fn restore(&mut self, ctx: &egui::Context, text: &mut String, turn: AgentTurn) {
        let (typed, files) = attach::split_inlined(&turn.text);
        if !typed.trim().is_empty() {
            if !text.trim().is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(typed.trim());
        }
        for file in files {
            self.attachments.push(Attachment {
                name: file.name.to_string(),
                body: Body::Text(file.body.to_string()),
                thumb: None,
            });
        }
        for image in turn.images {
            let Some(data) = image.data else { continue };
            match base64::engine::general_purpose::STANDARD.decode(data.trim()) {
                Ok(bytes) => attach::spawn_prepare(ctx, &self.tx, image.name, bytes),
                Err(e) => toast(format!("{} could not be restored: {e}", image.name)),
            }
        }
    }

    fn watch_paste(&mut self, ui: &Ui, focused: bool) {
        if !focused {
            return;
        }
        let now = ui.input(|i| i.time);
        let (pasted_text, released_v) = ui.input(|i| {
            let text = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
            let v = i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command)
            });
            (text, v)
        });
        if pasted_text {
            self.text_pasted_at = Some(now);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if released_v
            && self
                .text_pasted_at
                .is_none_or(|t| now - t > TEXT_PASTE_GRACE)
        {
            attach::paste_image(ui.ctx(), &self.tx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = released_v;
            for (name, bytes) in attach::web_paste::take() {
                attach::spawn_prepare(ui.ctx(), &self.tx, name, bytes);
            }
        }
    }

    /// Icon buttons in columns `height` tall: Send now over Stop while `busy`, then attach over Send or Queue.
    fn controls(
        &mut self,
        ui: &mut Ui,
        text: &mut String,
        busy: bool,
        height: f32,
    ) -> Option<ComposerAction> {
        let mut action = None;
        let ready = self.reading == 0 && !(text.trim().is_empty() && self.attachments.is_empty());
        let gap = ui.spacing().item_spacing.y;
        let half = vec2(SIDE_BUTTON_W, ((height - gap) / 2.0).max(0.0));
        if busy {
            ui.vertical(|ui| {
                if ui
                    .add_enabled(ready, Button::new(icons::SEND_NOW).min_size(half))
                    .on_hover_text("Send now: tell the agent while it works")
                    .clicked()
                {
                    action = self.submit(text, "steer");
                }
                if ui
                    .add(
                        Button::new(RichText::new(icons::STOP).color(theme::warn(ui)))
                            .min_size(half),
                    )
                    .on_hover_text(
                        "Stop the running turn; queued messages wait until you resume them",
                    )
                    .clicked()
                {
                    action = Some(ComposerAction::Stop);
                }
            });
        }
        ui.vertical(|ui| {
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            let send_size = {
                if ui
                    .add(Button::new(icons::PAPERCLIP).min_size(half))
                    .on_hover_text(
                        "Attach pictures or text files; you can also paste a screenshot or drop files here",
                    )
                    .clicked()
                {
                    attach::pick_files(ui.ctx(), &self.tx);
                }
                half
            };
            #[cfg(any(target_os = "ios", target_os = "android"))]
            let send_size = vec2(SIDE_BUTTON_W, height);
            let (icon, hover, kind) = if busy {
                (
                    icons::QUEUE,
                    "Queue: send when the agent finishes this turn (Enter)",
                    "queue",
                )
            } else {
                (icons::SEND, "Send (Enter)", "start")
            };
            if ui
                .add_enabled(ready, Button::new(icon).min_size(send_size))
                .on_hover_text(hover)
                .clicked()
            {
                action = self.submit(text, kind);
            }
        });
        action
    }

    fn submit(&mut self, text: &mut String, kind: &'static str) -> Option<ComposerAction> {
        if text.trim().is_empty() && self.attachments.is_empty() {
            return None;
        }
        if self.reading > 0 {
            toast("Wait for the attachments to finish reading.".into());
            return None;
        }
        let (message, images) = attach::compose(text, &self.attachments);
        let staged = attach::remember_sent(&self.attachments);
        text.clear();
        self.attachments.clear();
        Some(ComposerAction::Send {
            kind,
            text: message,
            images,
            staged,
        })
    }

    fn attachment_strip(&mut self, ui: &mut Ui) {
        if self.attachments.is_empty() && self.reading == 0 {
            return;
        }
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, a) in self.attachments.iter().enumerate() {
                ui.push_id(i, |ui| {
                    if attachment_chip(ui, a) {
                        remove = Some(i);
                    }
                });
            }
            if self.reading > 0 {
                ui.add(Spinner::new().size(12.0));
                ui.label(RichText::new("Reading\u{2026}").small().weak());
            }
        });
        if let Some(i) = remove {
            self.attachments.remove(i);
        }
    }
}

/// Removes an Enter press with no modifier from this frame's input; true when there was one.
fn take_plain_enter(input: &mut egui::InputState) -> bool {
    let at = input.events.iter().position(|e| {
        matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none())
    });
    at.map(|n| input.events.remove(n)).is_some()
}

/// Records `id` as the composer the technician last used.
fn mark_active(ui: &Ui, id: Id) {
    ui.data_mut(|d| d.insert_temp(Id::new(ACTIVE_KEY), id));
}

/// One attachment as a thumbnail or a file chip with a remove button; true when removed.
fn attachment_chip(ui: &mut Ui, a: &Attachment) -> bool {
    let mut removed = false;
    egui::Frame::group(ui.style())
        .inner_margin(2.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                match (&a.body, &a.thumb) {
                    (
                        Body::Image {
                            width,
                            height,
                            bytes,
                            ..
                        },
                        Some(tex),
                    ) => {
                        let details = format!(
                            "{} \u{00b7} {width}\u{00d7}{height} \u{00b7} {} KB",
                            a.name,
                            bytes.len().div_ceil(1024)
                        );
                        ui.add(
                            egui::Image::from_texture(tex)
                                .max_size(vec2(THUMB, THUMB))
                                .corner_radius(4.0),
                        )
                        .on_hover_ui(|ui| {
                            ui.add(egui::Image::from_texture(tex).max_width(320.0));
                            ui.label(RichText::new(details).small().weak());
                        });
                    }
                    (Body::Image { .. }, None) => {
                        ui.label(RichText::new(format!("{} {}", icons::IMAGE, a.name)).small());
                    }
                    (Body::Text(content), _) => {
                        ui.label(
                            RichText::new(format!(
                                "{} {} \u{00b7} {} KB",
                                icons::FILE_TEXT,
                                a.name,
                                content.len().div_ceil(1024)
                            ))
                            .small(),
                        );
                    }
                }
                removed = ui
                    .small_button(icons::CLOSE)
                    .on_hover_text("Remove")
                    .clicked();
            });
        });
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, Event, PointerButton, Pos2, RawInput, pos2};

    /// One frame of a focused composer in a 420 px viewport, fed `events`.
    fn frame(
        ctx: &Context,
        composer: &mut Composer,
        text: &mut String,
        busy: bool,
        events: Vec<Event>,
    ) -> Option<ComposerAction> {
        let id = Id::new("composer_test");
        let input = RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 300.0))),
            ..Default::default()
        };
        let mut action = None;
        let mut out = ctx.run_ui(input, |ui| {
            ui.memory_mut(|m| m.request_focus(id.with("text")));
            action = composer.show(ui, id, text, busy, true, 100.0);
        });
        out.textures_delta.clear();
        action
    }

    fn enter(modifiers: Modifiers) -> Event {
        Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn enter_sends_when_idle_and_queues_while_busy() {
        let ctx = Context::default();
        let mut composer = Composer::default();
        let mut text = "check the disks".to_string();
        assert_eq!(
            frame(&ctx, &mut composer, &mut text, false, Vec::new()),
            None
        );
        let sent = frame(
            &ctx,
            &mut composer,
            &mut text,
            false,
            vec![enter(Modifiers::NONE)],
        );
        assert!(
            matches!(&sent, Some(ComposerAction::Send { kind: "start", text, .. }) if text == "check the disks"),
            "{sent:?}"
        );
        assert!(text.is_empty(), "the box is cleared once sent");

        text = "then the event log".to_string();
        let queued = frame(
            &ctx,
            &mut composer,
            &mut text,
            true,
            vec![enter(Modifiers::NONE)],
        );
        assert!(
            matches!(queued, Some(ComposerAction::Send { kind: "queue", .. })),
            "{queued:?}"
        );
    }

    #[test]
    fn shift_enter_and_an_empty_box_send_nothing() {
        let ctx = Context::default();
        let mut composer = Composer::default();
        let mut text = "line one".to_string();
        frame(&ctx, &mut composer, &mut text, false, Vec::new());
        assert_eq!(
            frame(
                &ctx,
                &mut composer,
                &mut text,
                false,
                vec![enter(Modifiers::SHIFT)]
            ),
            None
        );
        let mut empty = String::new();
        assert_eq!(
            frame(
                &ctx,
                &mut composer,
                &mut empty,
                false,
                vec![enter(Modifiers::NONE)]
            ),
            None
        );
    }

    #[test]
    fn a_send_carries_inlined_files_and_leaves_no_attachment_behind() {
        let mut composer = Composer::default();
        composer.attachments.push(Attachment {
            name: "notes.txt".into(),
            body: Body::Text("alpha".into()),
            thumb: None,
        });
        let mut text = "see attached".to_string();
        let sent = composer.submit(&mut text, "steer");
        match sent {
            Some(ComposerAction::Send {
                kind,
                text: message,
                images,
                staged,
            }) => {
                assert_eq!(kind, "steer");
                assert_eq!(message, "see attached\n\n**notes.txt**\n```txt\nalpha\n```");
                assert!(images.is_empty() && staged.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert!(composer.attachments.is_empty() && text.is_empty());
        assert_eq!(composer.submit(&mut text, "start"), None);
    }

    /// Presses and releases the primary button at `pos` over two frames.
    fn click(
        ctx: &Context,
        composer: &mut Composer,
        text: &mut String,
        busy: bool,
        pos: Pos2,
    ) -> Option<ComposerAction> {
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let pressed = frame(
            ctx,
            composer,
            text,
            busy,
            vec![Event::PointerMoved(pos), button(true)],
        );
        let released = frame(ctx, composer, text, busy, vec![button(false)]);
        pressed.or(released)
    }

    /// The text box after one frame, and the x centre of each button column beside it.
    fn columns(
        ctx: &Context,
        composer: &mut Composer,
        text: &mut String,
        busy: bool,
    ) -> (Rect, [f32; 2]) {
        frame(ctx, composer, text, busy, Vec::new());
        let rect = ctx
            .read_response(Id::new("composer_test").with("text"))
            .expect("the text box was drawn")
            .rect;
        let gap = ctx.global_style().spacing.item_spacing.x;
        let first = rect.right() + gap + SIDE_BUTTON_W / 2.0;
        (rect, [first, first + gap + SIDE_BUTTON_W])
    }

    #[test]
    fn send_sits_beside_the_text_box_at_its_bottom() {
        let ctx = Context::default();
        let mut composer = Composer::default();
        let mut text = "check the disks".to_string();
        let (rect, [column, _]) = columns(&ctx, &mut composer, &mut text, false);
        assert!(
            column + SIDE_BUTTON_W / 2.0 <= 420.5,
            "the column fits the viewport: {rect:?}"
        );
        let sent = click(
            &ctx,
            &mut composer,
            &mut text,
            false,
            pos2(column, rect.bottom() - 3.0),
        );
        assert!(
            matches!(sent, Some(ComposerAction::Send { kind: "start", .. })),
            "{sent:?}"
        );
    }

    #[test]
    fn while_busy_the_columns_hold_send_now_over_stop_and_queue_at_the_bottom() {
        let ctx = Context::default();
        let mut composer = Composer::default();
        let mut text = "and the event log".to_string();
        let (rect, [left, right]) = columns(&ctx, &mut composer, &mut text, true);
        let steer = click(
            &ctx,
            &mut composer,
            &mut text,
            true,
            pos2(left, rect.top() + 3.0),
        );
        assert!(
            matches!(steer, Some(ComposerAction::Send { kind: "steer", .. })),
            "{steer:?}"
        );
        text = "then this".to_string();
        let queued = click(
            &ctx,
            &mut composer,
            &mut text,
            true,
            pos2(right, rect.bottom() - 3.0),
        );
        assert!(
            matches!(queued, Some(ComposerAction::Send { kind: "queue", .. })),
            "{queued:?}"
        );
        let stop = click(
            &ctx,
            &mut composer,
            &mut text,
            true,
            pos2(left, rect.bottom() - 3.0),
        );
        assert_eq!(stop, Some(ComposerAction::Stop));
    }

    #[test]
    fn a_send_waits_for_attachments_still_being_read() {
        let mut composer = Composer {
            reading: 1,
            ..Composer::default()
        };
        let mut text = "hi".to_string();
        assert_eq!(composer.submit(&mut text, "start"), None);
        assert_eq!(text, "hi", "the message stays in the box");
    }
}

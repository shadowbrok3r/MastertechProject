//! Chat transcript rows: author-tinted bubbles, a header strip, collapsible machinery rows and a dark text plate.

mod json_tree;
pub(crate) mod markdown;
mod shell;

use std::borrow::Cow;
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone, Utc};
use eframe::egui::{
    self, Align, Align2, Atom, Color32, CornerRadius, FontId, Frame, Id, Label, Layout, Margin,
    RichText, Sense, Spinner, Stroke, TextFormat, TextStyle, TextWrapMode, Ui, UiBuilder,
    text::LayoutJob, vec2,
};
use serde_json::Value;

use super::mtech_glass::{edge, lift, pane};
use super::{icons, theme};
use shell::ShellColors;

/// Width reserved for a header's copy button.
const COPY_W: f32 = 30.0;
/// Width reserved for the streaming spinner.
const SPIN_W: f32 = 20.0;
const HEADER_H: f32 = 22.0;
const ROW_GAP: f32 = 6.0;
/// Seconds the copy button shows a check after a press.
const COPIED_SECS: f64 = 1.5;
/// Longest monospace payload drawn in a row.
const MONO_MAX_CHARS: usize = 16_000;
/// Lowest ink intensity on a dark theme, and highest on a light one.
const DARK_INK_MIN: f32 = 0.58;
const LIGHT_INK_MAX: f32 = 0.45;
/// Longest string member shown in a one-line JSON summary.
const SUMMARY_VALUE_CHARS: usize = 60;

/// What a transcript row holds; the kind picks its tint, icon and whether it collapses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatKind {
    User,
    Agent,
    Reasoning,
    Tool,
    Command,
    FileChange,
    Approval,
    Error,
}

impl ChatKind {
    /// True for rows drawn collapsed behind a framed one-line summary.
    pub fn collapsible(self) -> bool {
        matches!(
            self,
            Self::Reasoning | Self::Tool | Self::Command | Self::FileChange | Self::Approval
        )
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::User => icons::USER,
            Self::Agent => icons::ROBOT,
            Self::Reasoning => icons::LIGHTBULB,
            Self::Tool => icons::WRENCH,
            Self::Command => icons::TERMINAL,
            Self::FileChange => icons::FILE_TEXT,
            Self::Approval => icons::LOCK,
            Self::Error => icons::STATUS_WARN,
        }
    }
}

/// Colours and fonts for chat rows, resolved from the active theme.
#[derive(Clone, Debug)]
pub struct ChatStyle {
    pub(crate) text: Color32,
    pub(crate) weak: Color32,
    pub(crate) strong: Color32,
    pub(crate) link: Color32,
    /// Legible inks of the primary, secondary and tertiary accent roles.
    pub(crate) primary: Color32,
    pub(crate) secondary: Color32,
    pub(crate) tertiary: Color32,
    pub(crate) error: Color32,
    /// Full-strength hues the row tints are cut from.
    pub(crate) primary_hue: Color32,
    pub(crate) secondary_hue: Color32,
    pub(crate) tertiary_hue: Color32,
    pub(crate) error_hue: Color32,
    pub(crate) plate: Color32,
    pub(crate) code_plate: Color32,
    pub(crate) code_chip: Color32,
    pub(crate) rim: Color32,
    pub(crate) rim_bright: Color32,
    pub(crate) radius: u8,
    pub(crate) body: FontId,
    pub(crate) mono: FontId,
    pub(crate) small: FontId,
    pub(crate) label: FontId,
}

impl ChatStyle {
    pub fn from_ui(ui: &Ui) -> Self {
        let v = ui.visuals();
        let dark = v.dark_mode;
        let text = v.text_color();
        let hue = |c: Color32| hue_of(c).unwrap_or(text);
        let primary_hue = hue(theme::accent(ui));
        let secondary_hue = hue(theme::accent_secondary(ui));
        let tertiary_hue = hue(theme::warn(ui));
        let error_hue = hue(theme::error(ui));
        let (plate, code_plate) = if dark {
            (
                Color32::from_black_alpha(150),
                Color32::from_black_alpha(110),
            )
        } else {
            (
                Color32::from_white_alpha(170),
                Color32::from_black_alpha(16),
            )
        };
        let style = ui.style();
        Self {
            text,
            weak: v.weak_text_color(),
            strong: v.strong_text_color(),
            link: legible(hue(v.hyperlink_color), dark),
            primary: legible(primary_hue, dark),
            secondary: legible(secondary_hue, dark),
            tertiary: legible(tertiary_hue, dark),
            error: legible(error_hue, dark),
            primary_hue,
            secondary_hue,
            tertiary_hue,
            error_hue,
            plate,
            code_plate,
            code_chip: pane(primary_hue, 0.18),
            rim: text.gamma_multiply(0.18),
            rim_bright: text.gamma_multiply(0.3),
            radius: v.widgets.noninteractive.corner_radius.nw.clamp(3, 8),
            body: TextStyle::Body.resolve(style),
            mono: TextStyle::Monospace.resolve(style),
            small: TextStyle::Small.resolve(style),
            label: TextStyle::Button.resolve(style),
        }
    }

    /// Tint hue and label ink of a row kind.
    pub(crate) fn accent(&self, kind: ChatKind) -> (Color32, Color32) {
        match kind {
            ChatKind::User | ChatKind::Command | ChatKind::Approval => {
                (self.tertiary_hue, self.tertiary)
            }
            ChatKind::Agent | ChatKind::Tool => (self.primary_hue, self.primary),
            ChatKind::Reasoning | ChatKind::FileChange => (self.secondary_hue, self.secondary),
            ChatKind::Error => (self.error_hue, self.error),
        }
    }

    pub(crate) fn shell_colors(&self, base: Color32) -> ShellColors {
        ShellColors {
            command: self.tertiary,
            flag: self.primary,
            string: self.secondary,
            number: self.tertiary,
            operator: self.secondary,
            base,
        }
    }
}

/// Full-strength hue of a possibly translucent or additive colour; `None` for transparent black.
fn hue_of(c: Color32) -> Option<Color32> {
    let [r, g, b, a] = c.to_array();
    if a == 255 {
        return Some(c);
    }
    let max = r.max(g).max(b);
    if max == 0 {
        return None;
    }
    // Scales additive colours by their brightest channel instead of by alpha.
    let scale = 255.0 / f32::from(if max > a { max } else { a });
    let ch = |v: u8| (f32::from(v) * scale).round().min(255.0) as u8;
    Some(Color32::from_rgb(ch(r), ch(g), ch(b)))
}

/// Moves a hue to at least `DARK_INK_MIN` intensity on dark themes, at most `LIGHT_INK_MAX` on light ones.
fn legible(c: Color32, dark: bool) -> Color32 {
    let i = c.intensity();
    if dark && i < DARK_INK_MIN {
        lift(c, (DARK_INK_MIN - i) / (1.0 - i))
    } else if !dark && i > LIGHT_INK_MAX {
        let t = LIGHT_INK_MAX / i;
        let ch = |v: u8| (f32::from(v) * t).round() as u8;
        Color32::from_rgb(ch(c.r()), ch(c.g()), ch(c.b()))
    } else {
        c
    }
}

/// Width a row's content may use: never more than the visible viewport.
fn wrap_width(ui: &Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().width()).max(48.0)
}

/// Runs `add` with buttons filled and outlined by `hue` in every interaction state.
fn tinted<R>(ui: &mut Ui, hue: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        let w = &mut ui.style_mut().visuals.widgets;
        for (state, fill, outline) in [
            (&mut w.inactive, 0.10, 0.30),
            (&mut w.hovered, 0.18, 0.62),
            (&mut w.active, 0.24, 0.85),
        ] {
            state.weak_bg_fill = pane(hue, fill);
            state.bg_stroke = Stroke::new(1.0, edge(hue, outline));
        }
        add(ui)
    })
    .inner
}

fn plate(style: &ChatStyle) -> Frame {
    Frame::new()
        .fill(style.plate)
        .stroke(Stroke::new(1.0, style.rim))
        .corner_radius(CornerRadius::same(style.radius))
        .inner_margin(Margin::symmetric(8, 6))
}

/// One transcript row: a header strip and a body on the text plate.
#[must_use = "call `show` to draw the row"]
pub struct ChatRow<'a> {
    kind: ChatKind,
    key: &'a str,
    label: &'a str,
    time: Option<String>,
    badge: Option<(String, Color32)>,
    summary: Option<String>,
    copy: Option<&'a str>,
    streaming: bool,
    default_open: bool,
    has_body: bool,
    nested: bool,
}

impl<'a> ChatRow<'a> {
    /// A row keyed by `key`, which must be unique and stable within its scope.
    pub fn new(kind: ChatKind, key: &'a str, label: &'a str) -> Self {
        Self {
            kind,
            key,
            label,
            time: None,
            badge: None,
            summary: None,
            copy: None,
            streaming: false,
            default_open: false,
            has_body: true,
            nested: false,
        }
    }

    pub fn time(mut self, time: Option<String>) -> Self {
        self.time = time;
        self
    }

    /// Short coloured status shown with the label.
    pub fn badge(mut self, text: impl Into<String>, color: Color32) -> Self {
        self.badge = Some((text.into(), color));
        self
    }

    /// One line shown in the header while a collapsible row is closed.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Text the copy button copies; without it `show` only reports the press.
    pub fn copy(mut self, text: &'a str) -> Self {
        self.copy = Some(text);
        self
    }

    /// Shows a spinner while the row's text is still arriving.
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Open state of a collapsible row until the technician toggles it.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn has_body(mut self, has_body: bool) -> Self {
        self.has_body = has_body;
        self
    }

    /// Draws only the header and an indented body, for rows inside another row's plate.
    pub fn nested(mut self, nested: bool) -> Self {
        self.nested = nested;
        self
    }

    /// Draws the row; `body` runs on the plate while open. True when the copy button was pressed.
    pub fn show(
        self,
        ui: &mut Ui,
        style: &ChatStyle,
        scope: Id,
        body: impl FnOnce(&mut Ui, Id),
    ) -> bool {
        let id = scope.with(self.key);
        let open_id = id.with("open");
        let collapsible = self.kind.collapsible();
        let open = !collapsible
            || ui
                .data(|d| d.get_temp::<bool>(open_id))
                .unwrap_or(self.default_open);
        let width = wrap_width(ui);
        let mut toggled = false;
        let mut copied = false;
        ui.scope_builder(
            UiBuilder::new().id(id).layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_max_width(width);
                if self.nested {
                    (toggled, copied) = self.header(ui, style, id, collapsible, open);
                    if open && self.has_body {
                        ui.indent(id.with("body"), |ui| body(ui, id));
                    }
                    return;
                }
                self.frame(style).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    (toggled, copied) = self.header(ui, style, id, collapsible, open);
                    if open && self.has_body {
                        plate(style).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            body(ui, id);
                        });
                    }
                });
            },
        );
        if toggled {
            ui.data_mut(|d| d.insert_temp(open_id, !open));
        }
        if copied && let Some(text) = self.copy {
            ui.ctx().copy_text(text.to_string());
        }
        ui.add_space(if self.nested { 2.0 } else { ROW_GAP });
        copied
    }

    fn frame(&self, style: &ChatStyle) -> Frame {
        let frame = Frame::new()
            .corner_radius(CornerRadius::same(style.radius))
            .inner_margin(Margin::same(6));
        if self.kind == ChatKind::User {
            let (hue, _) = style.accent(self.kind);
            frame
                .fill(pane(hue, 0.10))
                .stroke(Stroke::new(1.0, edge(hue, 0.5)))
        } else {
            frame.stroke(Stroke::new(1.0, style.rim))
        }
    }

    /// Draws the header strip; returns whether the toggle and the copy button were pressed.
    fn header(
        &self,
        ui: &mut Ui,
        style: &ChatStyle,
        id: Id,
        collapsible: bool,
        open: bool,
    ) -> (bool, bool) {
        let (hue, ink) = style.accent(self.kind);
        let mut toggled = false;
        let mut copied = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let tail = COPY_W + if self.streaming { SPIN_W } else { 0.0 };
            let head_w = (ui.available_width() - tail).max(48.0);
            let job = self.header_job(style, ink, collapsible.then_some(open));
            if collapsible {
                let button =
                    egui::Button::new((job, Atom::grow())).wrap_mode(TextWrapMode::Truncate);
                toggled = tinted(ui, hue, |ui| ui.add_sized([head_w, HEADER_H], button)).clicked();
            } else {
                ui.allocate_ui_with_layout(
                    vec2(head_w, HEADER_H),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(Label::new(job).truncate());
                    },
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                copied = copy_button(ui, style, id);
                if self.streaming {
                    ui.add(Spinner::new().size(14.0).color(ink));
                }
            });
        });
        (toggled, copied)
    }

    /// Marker, icon and label in the kind's ink, then badge, time and, while closed, the summary.
    fn header_job(&self, style: &ChatStyle, ink: Color32, marker: Option<bool>) -> LayoutJob {
        let fmt = |font: &FontId, color: Color32| TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        };
        let mut job = LayoutJob::default();
        if let Some(open) = marker {
            job.append(
                if open {
                    icons::CHEV_OPEN
                } else {
                    icons::CHEV_CLOSED
                },
                0.0,
                fmt(&style.label, ink),
            );
            job.append(" ", 0.0, fmt(&style.label, ink));
        }
        job.append(self.kind.icon(), 0.0, fmt(&style.label, ink));
        job.append(" ", 0.0, fmt(&style.label, ink));
        job.append(self.label, 0.0, fmt(&style.label, ink));
        if let Some((badge, color)) = &self.badge {
            job.append(badge, 8.0, fmt(&style.small, *color));
        }
        if let Some(time) = &self.time {
            job.append(time, 8.0, fmt(&style.small, style.weak));
        }
        if marker == Some(false)
            && let Some(summary) = self.summary.as_deref().filter(|s| !s.is_empty())
        {
            job.append(summary, 10.0, fmt(&style.label, style.text));
        }
        job
    }
}

/// The header's copy button, showing a check for a moment after a press; true when pressed.
fn copy_button(ui: &mut Ui, style: &ChatStyle, id: Id) -> bool {
    let at_id = id.with("copied_at");
    let now = ui.input(|i| i.time);
    let since = ui.data(|d| d.get_temp::<f64>(at_id)).map(|t| now - t);
    let recent = since.is_some_and(|s| s < COPIED_SECS);
    let glyph = if recent { icons::CHECK } else { icons::COPY };
    let button = egui::Button::new(
        RichText::new(glyph)
            .font(style.label.clone())
            .color(style.weak),
    );
    let pressed = ui
        .add_sized([COPY_W - 4.0, HEADER_H], button)
        .on_hover_text("Copy")
        .clicked();
    if pressed {
        ui.data_mut(|d| d.insert_temp(at_id, now));
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(COPIED_SECS));
    } else if let Some(s) = since.filter(|_| recent) {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64((COPIED_SECS - s).max(0.0)));
    }
    pressed
}

/// A turn boundary: a centred caption on a hairline.
pub fn divider(ui: &mut Ui, style: &ChatStyle, caption: &str) {
    ui.add_space(4.0);
    let galley = ui
        .painter()
        .layout_no_wrap(caption.to_string(), style.small.clone(), style.weak);
    let (rect, _) =
        ui.allocate_exact_size(vec2(wrap_width(ui), galley.size().y + 2.0), Sense::hover());
    let text = Align2::CENTER_CENTER.align_size_within_rect(galley.size(), rect);
    let stroke = Stroke::new(1.0, style.rim_bright);
    let y = rect.center().y;
    if text.left() - 8.0 > rect.left() {
        ui.painter()
            .hline(rect.left()..=text.left() - 8.0, y, stroke);
        ui.painter()
            .hline(text.right() + 8.0..=rect.right(), y, stroke);
    }
    ui.painter().galley(text.min, galley, style.weak);
    ui.add_space(2.0);
}

/// A marker line outside any row: an info glyph, weak wrapped text and the time.
pub fn notice(ui: &mut Ui, style: &ChatStyle, text: &str, time: Option<&str>) {
    let fmt = |color: Color32| TextFormat {
        font_id: style.small.clone(),
        color,
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width(ui);
    job.append(icons::INFO, 0.0, fmt(style.weak));
    job.append(text.trim(), 6.0, fmt(style.weak));
    if let Some(time) = time {
        job.append(time, 8.0, fmt(style.weak.gamma_multiply(0.7)));
    }
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.add(Label::new(galley));
    ui.add_space(2.0);
}

/// Markdown body in `ink`; trees and code blocks are keyed under `id`.
pub fn markdown(ui: &mut Ui, style: &ChatStyle, text: &str, ink: Color32, id: Id) {
    markdown::show(ui, text, style, ink, id);
}

/// A tool payload: JSON as a tree, a sentence ending in JSON as both, anything else monospace.
pub fn payload(ui: &mut Ui, style: &ChatStyle, text: &str, ink: Color32, id: Id) {
    if let Some(values) = markdown::cached_json(ui.ctx(), text) {
        for (n, v) in values.iter().enumerate() {
            json_tree::show(ui, id.with(n), v, style);
        }
        return;
    }
    if let Some((head, values)) = markdown::split_json(text) {
        mono_text(ui, style, head, ink);
        for (n, v) in values.iter().enumerate() {
            json_tree::show(ui, id.with(n), v, style);
        }
        return;
    }
    mono_text(ui, style, text, ink);
}

/// A JSON value as a collapsible tree keyed under `id`.
pub fn json(ui: &mut Ui, style: &ChatStyle, value: &Value, id: Id) {
    json_tree::show(ui, id, value, style);
}

/// A fenced code block on the darker inner plate.
pub fn code(ui: &mut Ui, style: &ChatStyle, lang: &str, code: &str, id: Id) {
    markdown::code_block(ui, lang, code, style, style.text, id);
}

/// A `$ command` line coloured by its parts.
pub fn shell_line(ui: &mut Ui, style: &ChatStyle, line: &str) {
    markdown::shell_line(ui, line, style, style.text);
}

/// Monospace text in `ink`, cut at [`MONO_MAX_CHARS`] with a note of what was left out.
pub fn mono_text(ui: &mut Ui, style: &ChatStyle, text: &str, ink: Color32) {
    match text.char_indices().nth(MONO_MAX_CHARS) {
        None => markdown::mono(ui, text, style, ink),
        Some((cut, _)) => {
            markdown::mono(ui, &text[..cut], style, ink);
            let rest = text[cut..].chars().count();
            caption(
                ui,
                style,
                &format!("{rest} more characters; the copy button copies all of it."),
            );
        }
    }
}

/// A small weak heading inside a row body.
pub fn caption(ui: &mut Ui, style: &ChatStyle, text: &str) {
    ui.label(
        RichText::new(text)
            .font(style.small.clone())
            .color(style.weak),
    );
}

/// `s` cut to `max` characters with an ellipsis.
pub fn clip(s: &str, max: usize) -> Cow<'_, str> {
    match s.char_indices().nth(max) {
        None => Cow::Borrowed(s),
        Some((cut, _)) => Cow::Owned(format!("{}…", &s[..cut])),
    }
}

/// First meaningful line of a markdown body without markup, cut to `max` characters.
pub fn summary_line(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))
        .unwrap_or("");
    let line = line
        .trim_start_matches('#')
        .trim_start_matches('>')
        .trim_start();
    let line = markdown::strip_list_marker(line);
    let mut out = String::new();
    for word in line.split_whitespace() {
        let word = word.replace("**", "").replace('`', "");
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
        if out.chars().count() > max {
            break;
        }
    }
    clip(&out, max).into_owned()
}

/// One-line summary of a payload: `key=value` members for JSON, else its first line.
pub fn payload_summary(ctx: &egui::Context, text: &str, max: usize) -> String {
    match markdown::cached_json(ctx, text)
        .as_deref()
        .map(Vec::as_slice)
    {
        Some([only]) => json_summary(only, max),
        _ => clip(
            text.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or(""),
            max,
        )
        .into_owned(),
    }
}

/// One-line `key=value` preview of a JSON value's members, cut to `max` characters.
pub fn json_summary(v: &Value, max: usize) -> String {
    let Value::Object(map) = v else {
        return clip(&member(v), max).into_owned();
    };
    let mut out = String::new();
    for (k, v) in map {
        if out.chars().count() > max {
            break;
        }
        if !out.is_empty() {
            out.push_str("  ");
        }
        out.push_str(k);
        out.push('=');
        out.push_str(&member(v));
    }
    clip(&out, max).into_owned()
}

fn member(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let mut out = String::new();
            for word in s.split_whitespace() {
                if out.len() > SUMMARY_VALUE_CHARS {
                    break;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(word);
            }
            clip(&out, SUMMARY_VALUE_CHARS).into_owned()
        }
        Value::Object(o) => format!("{{{}}}", o.len()),
        Value::Array(a) => format!("[{}]", a.len()),
        other => other.to_string(),
    }
}

/// Tool name without the `mastertech__` namespace every Mastertech tool carries.
pub fn tool_label(name: &str) -> &str {
    let name = name.strip_prefix("mastertech__").unwrap_or(name).trim();
    if name.is_empty() { "tool" } else { name }
}

/// `HH:MM` for a time on `now`'s day, `Mon DD HH:MM` for any other day.
pub fn clock_label<Tz: TimeZone>(at: &DateTime<Tz>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let fmt = if at.date_naive() == now.date_naive() {
        "%H:%M"
    } else {
        "%b %d %H:%M"
    };
    at.format(fmt).to_string()
}

/// [`clock_label`] of a UTC instant in the local time zone.
pub fn local_clock(at: DateTime<Utc>, now: &DateTime<Local>) -> String {
    clock_label(&at.with_timezone(&Local), now)
}

/// `340 ms`, `1.2 s` or `3m 04s`.
pub fn duration_label(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms} ms");
    }
    let tenths = (ms + 50) / 100;
    if tenths < 600 {
        return format!("{}.{} s", tenths / 10, tenths % 10);
    }
    let secs = (ms + 500) / 1_000;
    format!("{}m {:02}s", secs / 60, secs % 60)
}

/// `Turn 3` for the broker's `t3` turn label.
pub fn turn_title(turn: Option<&str>) -> String {
    match turn {
        Some(t) => match t
            .strip_prefix('t')
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            Some(n) => format!("Turn {n}"),
            None => t.to_string(),
        },
        None => "Turn".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_machinery_rows_collapse() {
        for kind in [ChatKind::User, ChatKind::Agent, ChatKind::Error] {
            assert!(!kind.collapsible(), "{kind:?}");
        }
        for kind in [
            ChatKind::Reasoning,
            ChatKind::Tool,
            ChatKind::Command,
            ChatKind::FileChange,
            ChatKind::Approval,
        ] {
            assert!(kind.collapsible(), "{kind:?}");
        }
    }

    #[test]
    fn a_summary_is_the_first_meaningful_line_without_markup() {
        assert_eq!(
            summary_line("\n\n## **Plan**  for `today`\nsecond", 80),
            "Plan for today"
        );
        assert_eq!(summary_line("```json\n{\"a\": 1}\n```", 80), "{\"a\": 1}");
        assert_eq!(summary_line("- first item\n- second", 80), "first item");
        assert_eq!(summary_line("> quoted   words", 80), "quoted words");
        assert_eq!(summary_line("", 80), "");
        assert_eq!(summary_line("abcdefghij", 4), "abcd…");
    }

    #[test]
    fn a_json_summary_lists_members_and_counts_containers() {
        let v = json!({"a_client": "PC-1", "b_query": "SELECT *\n  FROM task", "c_limit": 5, "d_opts": {"a": 1}, "e_ids": [1, 2]});
        assert_eq!(
            json_summary(&v, 200),
            "a_client=PC-1  b_query=SELECT * FROM task  c_limit=5  d_opts={1}  e_ids=[2]"
        );
        assert_eq!(json_summary(&json!("plain"), 20), "plain");
        assert_eq!(json_summary(&json!({}), 20), "");
        assert!(json_summary(&json!({"k": "x".repeat(500)}), 30).ends_with('…'));
    }

    #[test]
    fn clipping_counts_characters_not_bytes() {
        assert_eq!(clip("héllo wörld", 5), "héllo…");
        assert_eq!(clip("short", 5), "short");
    }

    #[test]
    fn durations_read_in_the_largest_unit_that_stays_a_number() {
        assert_eq!(duration_label(340), "340 ms");
        assert_eq!(duration_label(1_234), "1.2 s");
        assert_eq!(duration_label(59_999), "1m 00s");
        assert_eq!(duration_label(125_000), "2m 05s");
    }

    #[test]
    fn the_clock_shows_the_date_only_for_another_day() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 24, 15, 0, 0)
            .single()
            .expect("valid time");
        let today = Utc
            .with_ymd_and_hms(2026, 9, 24, 9, 5, 0)
            .single()
            .expect("valid time");
        let before = Utc
            .with_ymd_and_hms(2026, 9, 21, 23, 59, 0)
            .single()
            .expect("valid time");
        assert_eq!(clock_label(&today, &now), "09:05");
        assert_eq!(clock_label(&before, &now), "Sep 21 23:59");
    }

    #[test]
    fn turn_labels_and_tool_names_read_as_words() {
        assert_eq!(turn_title(Some("t3")), "Turn 3");
        assert_eq!(turn_title(Some("warmup")), "warmup");
        assert_eq!(turn_title(None), "Turn");
        assert_eq!(tool_label("mastertech__get_client_info"), "get_client_info");
        assert_eq!(tool_label("  "), "tool");
    }

    #[test]
    fn hues_recover_full_strength_from_panes_and_additive_colours() {
        let violet = Color32::from_rgb(126, 108, 224);
        let recovered = hue_of(pane(violet, 0.32)).expect("a hue");
        for (a, b) in [
            (recovered.r(), 126),
            (recovered.g(), 108),
            (recovered.b(), 224),
        ] {
            assert!(a.abs_diff(b) <= 3, "{recovered:?}");
        }
        assert_eq!(
            hue_of(Color32::from_rgba_premultiplied(23, 64, 53, 27)),
            Some(Color32::from_rgb(92, 255, 211))
        );
        assert_eq!(hue_of(Color32::TRANSPARENT), None);
        assert_eq!(hue_of(Color32::RED), Some(Color32::RED));
    }

    #[test]
    fn inks_are_lifted_on_dark_themes_and_darkened_on_light_ones() {
        let dim = Color32::from_rgb(40, 60, 120);
        assert!(legible(dim, true).intensity() >= DARK_INK_MIN - 0.01);
        assert!(
            legible(Color32::from_rgb(120, 240, 232), false).intensity() <= LIGHT_INK_MAX + 0.01
        );
        let bright = Color32::from_rgb(120, 240, 232);
        assert_eq!(legible(bright, true), bright);
    }
}

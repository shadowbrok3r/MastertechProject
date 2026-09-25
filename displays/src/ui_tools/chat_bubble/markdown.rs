//! Chat markdown: headings, lists, quotes, rules, fenced code, inline spans and JSON trees.

use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Layout, Margin, RichText,
    Sense, Stroke, TextFormat, Ui,
    cache::{ComputerMut, FrameCache},
    text::LayoutJob,
    vec2,
};
use serde_json::Value;

use super::{ChatStyle, json_tree, shell, wrap_width};
use crate::ui_tools::icons;

/// Pixels each list nesting level is inset by.
const LIST_INDENT: f32 = 12.0;
/// Heading size as a multiple of body text, by level.
const HEADING_SCALE: [f32; 6] = [1.4, 1.25, 1.12, 1.05, 1.0, 1.0];

/// How a list item is numbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker<'a> {
    Bullet,
    Number(&'a str),
}

/// One block of a message body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block<'a> {
    Heading {
        level: usize,
        text: &'a str,
    },
    Line {
        depth: usize,
        text: &'a str,
    },
    Item {
        depth: usize,
        marker: Marker<'a>,
        text: &'a str,
    },
    Quote(&'a str),
    /// A `$ command` line.
    Shell(&'a str),
    Code {
        lang: &'a str,
        code: &'a str,
    },
    Rule,
    Blank,
}

/// How an inline run is styled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind<'a> {
    Plain,
    Bold,
    Italic,
    Code,
    Link(&'a str),
}

/// One styled run within a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span<'a> {
    pub text: &'a str,
    pub kind: SpanKind<'a>,
}

/// Lines of `text` with the byte offsets of their start and of the next line.
fn lines(text: &str) -> impl Iterator<Item = (&str, usize, usize)> {
    let mut pos = 0;
    text.split_inclusive('\n').map(move |raw| {
        let start = pos;
        pos += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        (line.strip_suffix('\r').unwrap_or(line), start, pos)
    })
}

/// Splits a message body into blocks; an unclosed fence runs to the end of the text.
pub fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut iter = lines(text);
    while let Some((line, _, after)) = iter.next() {
        if let Some(lang) = line.trim_start().strip_prefix("```") {
            let mut end = text.len();
            for (inner, start, _) in iter.by_ref() {
                if inner.trim_start().starts_with("```") {
                    end = start;
                    break;
                }
            }
            let code = text[after.min(end)..end].trim_end_matches(['\n', '\r']);
            out.push(Block::Code {
                lang: lang.trim(),
                code,
            });
            continue;
        }
        out.push(line_block(line));
    }
    out
}

fn line_block(line: &str) -> Block<'_> {
    let body = line.trim_start();
    let depth = indent_depth(&line[..line.len() - body.len()]);
    let body = body.trim_end();
    if body.is_empty() {
        return Block::Blank;
    }
    if let Some((level, text)) = heading(body) {
        return Block::Heading { level, text };
    }
    if is_rule(body) {
        return Block::Rule;
    }
    if let Some((marker, text)) = list_item(body) {
        return Block::Item {
            depth,
            marker,
            text,
        };
    }
    if let Some(text) = body.strip_prefix('>') {
        return Block::Quote(text.trim_start());
    }
    if body.starts_with("$ ") {
        return Block::Shell(body);
    }
    Block::Line { depth, text: body }
}

/// Nesting level of a line's leading whitespace, two columns per level and a tab as four.
fn indent_depth(ws: &str) -> usize {
    let cols: usize = ws.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
    (cols / 2).min(8)
}

fn heading(body: &str) -> Option<(usize, &str)> {
    let level = body.bytes().take_while(|b| *b == b'#').count();
    let rest = body.get(level..)?;
    ((1..=6).contains(&level) && rest.starts_with(' '))
        .then(|| (level, rest.trim().trim_end_matches('#').trim_end()))
}

/// Three or more of one of `-`, `*`, `_`, optionally spaced.
fn is_rule(body: &str) -> bool {
    let mut marks = body.chars().filter(|c| !c.is_whitespace());
    let Some(first) = marks.next().filter(|c| matches!(c, '-' | '*' | '_')) else {
        return false;
    };
    let mut count = 1;
    for c in marks {
        if c != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// `line` without a leading list marker.
pub(crate) fn strip_list_marker(line: &str) -> &str {
    list_item(line).map_or(line, |(_, text)| text)
}

fn list_item(body: &str) -> Option<(Marker<'_>, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = body.strip_prefix(bullet) {
            return Some((Marker::Bullet, rest.trim_start()));
        }
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &body[digits..];
    (rest.starts_with(". ") || rest.starts_with(") "))
        .then(|| (Marker::Number(&body[..=digits]), rest[2..].trim_start()))
}

/// Splits a line into code, bold, italic and link runs; unpaired markers stay literal.
pub fn inline_spans(line: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < line.len() {
        if let Some((consumed, span)) = styled_at(line, i) {
            if plain < i {
                out.push(Span {
                    text: &line[plain..i],
                    kind: SpanKind::Plain,
                });
            }
            out.push(span);
            i += consumed;
            plain = i;
            continue;
        }
        i += line[i..].chars().next().map_or(1, char::len_utf8);
    }
    if plain < line.len() {
        out.push(Span {
            text: &line[plain..],
            kind: SpanKind::Plain,
        });
    }
    out
}

/// The styled run opening at byte `i`, with the number of bytes it spans.
fn styled_at(line: &str, i: usize) -> Option<(usize, Span<'_>)> {
    let rest = &line[i..];
    if let Some(body) = rest.strip_prefix('`') {
        let n = body.find('`').filter(|n| *n > 0)?;
        return Some((
            n + 2,
            Span {
                text: &body[..n],
                kind: SpanKind::Code,
            },
        ));
    }
    if let Some(body) = rest.strip_prefix("**") {
        let n = closing(body, "**", false)?;
        return Some((
            n + 4,
            Span {
                text: &body[..n],
                kind: SpanKind::Bold,
            },
        ));
    }
    if rest.starts_with('[') {
        return link(rest);
    }
    let (marker, word_bound) = match rest.as_bytes().first() {
        Some(b'*') => ("*", false),
        Some(b'_') if !prev_is_word(line, i) => ("_", true),
        _ => return None,
    };
    let body = &rest[1..];
    let n = closing(body, marker, word_bound)?;
    Some((
        n + 2,
        Span {
            text: &body[..n],
            kind: SpanKind::Italic,
        },
    ))
}

/// Offset of a closing marker; the run has no edge whitespace and a `word_bound` closer ends a word.
fn closing(body: &str, marker: &str, word_bound: bool) -> Option<usize> {
    if body.chars().next().is_none_or(char::is_whitespace) {
        return None;
    }
    body.match_indices(marker).map(|(n, _)| n).find(|&n| {
        n > 0
            && body[..n]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
            && !(word_bound && next_is_word(body, n + marker.len()))
    })
}

fn link(rest: &str) -> Option<(usize, Span<'_>)> {
    let close = rest.find("](")?;
    let label = &rest[1..close];
    if label.is_empty() || label.contains('[') {
        return None;
    }
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let url = &after[..end];
    if url.is_empty() || url.contains(char::is_whitespace) {
        return None;
    }
    Some((
        close + 3 + end,
        Span {
            text: label,
            kind: SpanKind::Link(url),
        },
    ))
}

/// True when the character before byte `i` is part of a word.
fn prev_is_word(line: &str, i: usize) -> bool {
    line[..i]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
}

/// True when the character at byte `i` is part of a word.
fn next_is_word(line: &str, i: usize) -> bool {
    line.get(i..)
        .and_then(|s| s.chars().next())
        .is_some_and(char::is_alphanumeric)
}

/// Parses text that is nothing but JSON, allowing several values back to back.
pub fn as_json(text: &str) -> Option<Vec<Value>> {
    let t = text.trim();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    let mut stream = serde_json::Deserializer::from_str(t).into_iter::<Value>();
    let mut out = Vec::new();
    for v in stream.by_ref() {
        match v {
            Ok(v) => out.push(v),
            Err(_) => return repair_cut_json(t).map(|v| vec![v]),
        }
    }
    (stream.byte_offset() >= t.len() && !out.is_empty()).then_some(out)
}

/// True when `text` ends in the `…` a recorder appends to text it cut short.
pub fn ends_cut(text: &str) -> bool {
    text.trim_end().ends_with('\u{2026}')
}

/// JSON cut short with a trailing `…`, closed back up: the open string, then each open bracket.
fn repair_cut_json(text: &str) -> Option<Value> {
    let body = text.trim_end().strip_suffix('\u{2026}')?.trim_end();
    let mut open: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut last_comma: Option<(usize, Vec<char>)> = None;
    for (i, c) in body.char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => open.push('}'),
            '[' => open.push(']'),
            '}' | ']' => {
                open.pop();
            }
            ',' => last_comma = Some((i, open.clone())),
            _ => {}
        }
    }
    let mut whole = body.to_string();
    if in_string {
        if escaped {
            whole.pop();
        }
        whole.push('"');
    }
    whole.extend(open.iter().rev());
    serde_json::from_str(&whole).ok().or_else(|| {
        // Drops the member after the last comma when that member itself was cut.
        let (at, open) = last_comma?;
        let mut head = body[..at].to_string();
        head.extend(open.iter().rev());
        serde_json::from_str(&head).ok()
    })
}

/// The prose in front of a trailing JSON value, and the values themselves.
pub fn split_json(text: &str) -> Option<(&str, Vec<Value>)> {
    let start = text.find(['{', '['])?;
    let (head, rest) = text.split_at(start);
    Some((head.trim_end(), as_json(rest)?))
}

#[derive(Default)]
struct JsonParser;

impl ComputerMut<&str, Option<Arc<Vec<Value>>>> for JsonParser {
    fn compute(&mut self, text: &str) -> Option<Arc<Vec<Value>>> {
        as_json(text).map(Arc::new)
    }
}

type JsonCache = FrameCache<Option<Arc<Vec<Value>>>, JsonParser>;

/// [`as_json`], parsed once and kept while the same text is drawn every frame.
pub(crate) fn cached_json(ctx: &egui::Context, text: &str) -> Option<Arc<Vec<Value>>> {
    let t = text.trim_start();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    ctx.memory_mut(|m| m.caches.cache::<JsonCache>().get(text).clone())
}

/// Renders a message body in `ink`; trees and code blocks are keyed under `id`.
pub(crate) fn show(ui: &mut Ui, text: &str, style: &ChatStyle, ink: Color32, id: Id) {
    if let Some(values) = cached_json(ui.ctx(), text) {
        for (n, v) in values.iter().enumerate() {
            json_tree::show(ui, id.with(("json", n)), v, style);
        }
        return;
    }
    let mut blank = true;
    for (n, block) in blocks(text).into_iter().enumerate() {
        if block == Block::Blank {
            if !blank {
                ui.add_space(style.body.size * 0.35);
            }
            blank = true;
            continue;
        }
        blank = false;
        match block {
            Block::Heading { level, text } => {
                ui.add_space(2.0);
                let scale = HEADING_SCALE
                    .get(level.saturating_sub(1))
                    .copied()
                    .unwrap_or(1.0);
                let font = FontId::new(style.body.size * scale, style.body.family.clone());
                spans_label(ui, text, &font, style.strong, style);
            }
            Block::Line { depth, text } => indented(ui, depth, |ui| {
                spans_label(ui, text, &style.body, ink, style)
            }),
            Block::Item {
                depth,
                marker,
                text,
            } => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_space(6.0 + depth as f32 * LIST_INDENT);
                    let marker = match marker {
                        Marker::Bullet => "\u{2022}",
                        Marker::Number(n) => n,
                    };
                    ui.label(
                        RichText::new(marker)
                            .font(style.body.clone())
                            .color(style.tertiary),
                    );
                    spans_label(ui, text, &style.body, ink, style);
                });
            }
            Block::Quote(text) => {
                let quoted = ui.horizontal_top(|ui| {
                    ui.add_space(10.0);
                    spans_label(ui, text, &style.body, style.primary, style);
                });
                let r = quoted.response.rect;
                ui.painter().vline(
                    r.left() + 3.0,
                    r.y_range(),
                    Stroke::new(2.0, style.rim_bright),
                );
            }
            Block::Shell(line) => shell_line(ui, line, style, ink),
            Block::Code { lang, code } => {
                code_block(ui, lang, code, style, ink, id.with(("code", n)))
            }
            Block::Rule => rule(ui, style),
            Block::Blank => {}
        }
    }
}

fn indented(ui: &mut Ui, depth: usize, add: impl FnOnce(&mut Ui)) {
    if depth == 0 {
        add(ui);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(depth as f32 * LIST_INDENT);
        add(ui);
    });
}

/// Lays out one line of inline spans as a single wrapped job.
fn spans_label(ui: &mut Ui, line: &str, font: &FontId, color: Color32, style: &ChatStyle) {
    let mono = FontId::new(font.size, FontFamily::Monospace);
    let bold = if color == style.text {
        style.strong
    } else {
        color
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width(ui);
    let fmt = |font: &FontId, color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    for span in inline_spans(line) {
        match span.kind {
            SpanKind::Plain => job.append(span.text, 0.0, fmt(font, color)),
            SpanKind::Bold => job.append(span.text, 0.0, fmt(font, bold)),
            SpanKind::Italic => job.append(
                span.text,
                0.0,
                TextFormat {
                    italics: true,
                    ..fmt(font, color)
                },
            ),
            SpanKind::Code => job.append(
                span.text,
                0.0,
                TextFormat {
                    background: style.code_chip,
                    ..fmt(&mono, style.secondary)
                },
            ),
            SpanKind::Link(url) => {
                job.append(span.text, 0.0, fmt(font, style.link));
                if url != span.text {
                    job.append(&format!(" ({url})"), 0.0, fmt(font, style.weak));
                }
            }
        }
    }
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley));
}

pub(crate) fn shell_line(ui: &mut Ui, line: &str, style: &ChatStyle, ink: Color32) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width(ui);
    let cmd = match line.strip_prefix("$ ") {
        Some(rest) => {
            job.append(
                "$ ",
                0.0,
                TextFormat {
                    font_id: style.mono.clone(),
                    color: style.weak,
                    ..Default::default()
                },
            );
            rest
        }
        None => line,
    };
    shell::append(&mut job, cmd, &style.mono, &style.shell_colors(ink));
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley));
}

/// Monospace text in `ink`, wrapped to the available width.
pub(crate) fn mono(ui: &mut Ui, text: &str, style: &ChatStyle, ink: Color32) {
    let job = LayoutJob::simple(text.to_string(), style.mono.clone(), ink, wrap_width(ui));
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley));
}

pub(crate) fn code_block(
    ui: &mut Ui,
    lang: &str,
    code: &str,
    style: &ChatStyle,
    ink: Color32,
    id: Id,
) {
    let json_fence = lang.is_empty() || lang.eq_ignore_ascii_case("json");
    if json_fence && let Some(values) = cached_json(ui.ctx(), code) {
        for (n, v) in values.iter().enumerate() {
            json_tree::show(ui, id.with(n), v, style);
        }
        return;
    }
    Frame::new()
        .fill(style.code_plate)
        .stroke(Stroke::new(1.0, style.rim))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if lang.is_empty() { "code" } else { lang })
                        .font(style.small.clone())
                        .color(style.weak),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let copy =
                        egui::Button::new(RichText::new(icons::COPY).font(style.label.clone()))
                            .frame(false);
                    if ui.add(copy).on_hover_text("Copy code").clicked() {
                        ui.ctx().copy_text(code.to_string());
                    }
                });
            });
            let mut job = if shell::is_shell_lang(lang) {
                let mut job = LayoutJob::default();
                shell::append(&mut job, code, &style.mono, &style.shell_colors(ink));
                job
            } else if lang.is_empty() {
                LayoutJob::simple(code.to_string(), style.mono.clone(), ink, f32::INFINITY)
            } else {
                let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
                egui_extras::syntax_highlighting::highlight(
                    ui.ctx(),
                    ui.style(),
                    &theme,
                    code,
                    lang,
                )
            };
            job.wrap.max_width = wrap_width(ui);
            let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
            ui.add(egui::Label::new(galley));
        });
}

fn rule(ui: &mut Ui, style: &ChatStyle) {
    let (rect, _) = ui.allocate_exact_size(vec2(wrap_width(ui), 7.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, style.rim_bright),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds<'a>(spans: &[Span<'a>]) -> Vec<(&'a str, SpanKind<'a>)> {
        spans.iter().map(|s| (s.text, s.kind)).collect()
    }

    #[test]
    fn a_body_splits_into_headings_lists_quotes_rules_and_lines() {
        let text = "# Title\n\nSome **prose** here.\n- one\n  - nested\n12. twelve\n> quoted\n---\n$ ls -la\n   indented";
        assert_eq!(
            blocks(text),
            vec![
                Block::Heading {
                    level: 1,
                    text: "Title"
                },
                Block::Blank,
                Block::Line {
                    depth: 0,
                    text: "Some **prose** here."
                },
                Block::Item {
                    depth: 0,
                    marker: Marker::Bullet,
                    text: "one"
                },
                Block::Item {
                    depth: 1,
                    marker: Marker::Bullet,
                    text: "nested"
                },
                Block::Item {
                    depth: 0,
                    marker: Marker::Number("12."),
                    text: "twelve"
                },
                Block::Quote("quoted"),
                Block::Rule,
                Block::Shell("$ ls -la"),
                Block::Line {
                    depth: 1,
                    text: "indented"
                },
            ]
        );
    }

    #[test]
    fn a_fence_keeps_its_language_and_its_lines_verbatim() {
        let text = "before\n```rust\nfn main() {\n    let x = 1;\n}\n```\nafter";
        assert_eq!(
            blocks(text),
            vec![
                Block::Line {
                    depth: 0,
                    text: "before"
                },
                Block::Code {
                    lang: "rust",
                    code: "fn main() {\n    let x = 1;\n}"
                },
                Block::Line {
                    depth: 0,
                    text: "after"
                },
            ]
        );
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end_of_a_streaming_body() {
        assert_eq!(
            blocks("```json\n{\"a\": 1,\n"),
            vec![Block::Code {
                lang: "json",
                code: "{\"a\": 1,"
            }]
        );
        assert_eq!(
            blocks("text\n```"),
            vec![
                Block::Line {
                    depth: 0,
                    text: "text"
                },
                Block::Code { lang: "", code: "" }
            ]
        );
    }

    #[test]
    fn crlf_bodies_split_like_lf_ones() {
        assert_eq!(
            blocks("a\r\n```\r\nx\r\n```\r\nb"),
            vec![
                Block::Line {
                    depth: 0,
                    text: "a"
                },
                Block::Code {
                    lang: "",
                    code: "x"
                },
                Block::Line {
                    depth: 0,
                    text: "b"
                }
            ]
        );
    }

    #[test]
    fn near_misses_stay_plain_lines() {
        assert_eq!(
            line_block("#hashtag"),
            Block::Line {
                depth: 0,
                text: "#hashtag"
            }
        );
        assert_eq!(
            line_block("-no space"),
            Block::Line {
                depth: 0,
                text: "-no space"
            }
        );
        assert_eq!(
            line_block("--"),
            Block::Line {
                depth: 0,
                text: "--"
            }
        );
        assert_eq!(
            line_block("3.14 is pi"),
            Block::Line {
                depth: 0,
                text: "3.14 is pi"
            }
        );
        assert_eq!(line_block("* * *"), Block::Rule);
        assert_eq!(
            line_block("## Closing hashes ##"),
            Block::Heading {
                level: 2,
                text: "Closing hashes"
            }
        );
        assert_eq!(
            line_block("####### seven"),
            Block::Line {
                depth: 0,
                text: "####### seven"
            }
        );
    }

    #[test]
    fn inline_spans_style_code_bold_italic_and_links() {
        assert_eq!(
            kinds(&inline_spans(
                "plain `code` **bold** *it* see [docs](https://x.y/z) end"
            )),
            vec![
                ("plain ", SpanKind::Plain),
                ("code", SpanKind::Code),
                (" ", SpanKind::Plain),
                ("bold", SpanKind::Bold),
                (" ", SpanKind::Plain),
                ("it", SpanKind::Italic),
                (" see ", SpanKind::Plain),
                ("docs", SpanKind::Link("https://x.y/z")),
                (" end", SpanKind::Plain),
            ]
        );
    }

    #[test]
    fn underscores_inside_a_word_are_not_italics() {
        let v = inline_spans("query_surrealdb reads get_client_info.json");
        assert_eq!(
            kinds(&v),
            vec![(
                "query_surrealdb reads get_client_info.json",
                SpanKind::Plain
            )]
        );
        assert_eq!(
            kinds(&inline_spans("say _this_ loudly"))[1],
            ("this", SpanKind::Italic)
        );
    }

    #[test]
    fn unmatched_or_spaced_markers_stay_literal() {
        for text in [
            "2 * 3 = 6",
            "a **b",
            "rate: 5* ",
            "2 * 3 * 4",
            "[not a link]",
            "[a](has space)",
            "`",
            "``",
        ] {
            assert_eq!(
                kinds(&inline_spans(text)),
                vec![(text, SpanKind::Plain)],
                "{text}"
            );
        }
    }

    #[test]
    fn bold_wins_over_italic_and_multibyte_text_survives() {
        assert_eq!(
            kinds(&inline_spans("**both**")),
            vec![("both", SpanKind::Bold)]
        );
        let v = inline_spans("héllo **wörld** ünïcode");
        assert_eq!(
            kinds(&v),
            vec![
                ("héllo ", SpanKind::Plain),
                ("wörld", SpanKind::Bold),
                (" ünïcode", SpanKind::Plain)
            ]
        );
    }

    #[test]
    fn only_whole_json_is_treated_as_json() {
        assert_eq!(as_json(r#"  {"a":1}  "#).map(|v| v.len()), Some(1));
        assert_eq!(as_json("{\"a\":1}\n{\"b\":2}").map(|v| v.len()), Some(2));
        assert!(as_json("here is {\"a\":1}").is_none());
        assert!(as_json("{not json").is_none());
        assert!(as_json("{\"a\":1}\nthen some words").is_none());
    }

    #[test]
    fn json_cut_short_by_an_ellipsis_is_closed_back_up() {
        let v = as_json("{\"query\": \"select:a,b,cre\u{2026}").expect("open string");
        assert_eq!(v[0]["query"], "select:a,b,cre");
        let v = as_json("{\"cs\": \"D:1\", \"hostname\": \"D\"\u{2026}").expect("open object");
        assert_eq!(v[0]["hostname"], "D");
        let v = as_json("{\"a\": 1, \"b\": [1, 2\u{2026}").expect("open array");
        assert_eq!(v[0]["b"], serde_json::json!([1, 2]));
        let v = as_json("{\"a\": 1, \"b\"\u{2026}").expect("cut member dropped");
        assert_eq!(v[0], serde_json::json!({ "a": 1 }));
        assert!(as_json("{\"a\": tr\u{2026}").is_none());
        assert!(ends_cut("{\"a\": 1\u{2026} ") && !ends_cut("{\"a\": 1}"));
    }

    #[test]
    fn a_message_splits_into_its_sentence_and_its_json() {
        let (head, vs) =
            split_json(r#"tool failed: {"code":-32601,"message":"no"}"#).expect("split");
        assert_eq!(head, "tool failed:");
        assert_eq!(vs[0]["code"], -32601);
        assert!(split_json(r#"failed: {"a":1} and then some"#).is_none());
        assert!(split_json("no json here").is_none());
    }
}

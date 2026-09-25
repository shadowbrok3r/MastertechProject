//! Measuring, clipping, cleaning and wrapping styled text in terminal cells.

use std::borrow::Cow;
use std::iter::Peekable;
use std::str::Chars;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::terminal_mode::styling::glyphs;

/// Cells `s` occupies.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn cells(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Cells a run of spans occupies.
pub fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| width(&s.content)).sum()
}

/// `s` cut to `max` cells, ending in an ellipsis when anything was cut.
pub fn clip(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    clip_marked(s, max)
}

/// `s` cut to `max` cells with an ellipsis in the last cell.
fn clip_marked(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = cells(c);
        if used + w + 1 > max {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push_str(glyphs::ELLIPSIS);
    out
}

/// `text` with tabs as four spaces and without escape sequences, carriage returns or other control characters.
pub fn sanitize(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|c| c.is_control() && c != '\n') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            '\u{1b}' => skip_escape(&mut chars),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Consumes an escape sequence's tail: a CSI to its final byte, an OSC to BEL or ST, else one character.
fn skip_escape(chars: &mut Peekable<Chars<'_>>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
        Some(']') => {
            while let Some(c) = chars.next() {
                if c == '\u{7}' {
                    break;
                }
                if c == '\u{1b}' {
                    chars.next_if_eq(&'\\');
                    break;
                }
            }
        }
        _ => {}
    }
}

/// A line under construction: its spans, the cells they take and whether text follows the prefix.
struct Liner {
    spans: Vec<Span<'static>>,
    used: usize,
    text: bool,
}

impl Liner {
    fn new(prefix: &[Span<'static>]) -> Self {
        Self {
            spans: prefix.to_vec(),
            used: spans_width(prefix),
            text: false,
        }
    }

    fn push(&mut self, c: char, style: Style) {
        self.used += cells(c);
        self.text = true;
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push(c),
            _ => self.spans.push(Span::styled(c.to_string(), style)),
        }
    }

    fn finish(self) -> Line<'static> {
        Line::from(self.spans)
    }
}

/// A word, the style of the space before it and the cells it takes.
struct Word {
    gap: Style,
    parts: Vec<(Style, String)>,
    cells: usize,
}

/// Word-wraps styled runs to `width` cells, collapsing whitespace; lines after the first start with `rest`.
pub fn words<S: AsRef<str>>(
    runs: &[(Style, S)],
    width: usize,
    first: &[Span<'static>],
    rest: &[Span<'static>],
) -> Vec<Line<'static>> {
    let mut all: Vec<Word> = Vec::new();
    let mut open: Option<Word> = None;
    let mut gap = Style::default();
    for (style, text) in runs {
        for c in text.as_ref().chars() {
            if c.is_whitespace() {
                all.extend(open.take());
                gap = *style;
                continue;
            }
            let word = open.get_or_insert_with(|| Word {
                gap,
                parts: Vec::new(),
                cells: 0,
            });
            match word.parts.last_mut() {
                Some((s, t)) if s == style => t.push(c),
                _ => word.parts.push((*style, c.to_string())),
            }
            word.cells += cells(c);
        }
    }
    all.extend(open);

    let mut lines = Vec::new();
    let mut line = Liner::new(first);
    for word in all {
        if line.text && line.used + 1 + word.cells > width {
            lines.push(std::mem::replace(&mut line, Liner::new(rest)).finish());
        }
        if line.text {
            line.push(' ', word.gap);
        }
        for (style, text) in &word.parts {
            for c in text.chars() {
                if line.text && line.used + cells(c) > width {
                    lines.push(std::mem::replace(&mut line, Liner::new(rest)).finish());
                }
                line.push(c, *style);
            }
        }
    }
    lines.push(line.finish());
    lines
}

/// Wraps styled runs to `width` cells keeping every space, breaking after the last space that fits when there is one.
pub fn mono<S: AsRef<str>>(
    runs: &[(Style, S)],
    width: usize,
    first: &[Span<'static>],
    rest: &[Span<'static>],
) -> Vec<Line<'static>> {
    let chars: Vec<(char, Style)> = runs
        .iter()
        .flat_map(|(style, text)| text.as_ref().chars().map(move |c| (c, *style)))
        .collect();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut prefix = first;
    loop {
        let room = width.saturating_sub(spans_width(prefix)).max(1);
        let (mut end, mut used, mut brk, mut text) = (start, 0, None, false);
        while let Some(&(c, _)) = chars.get(end) {
            let w = cells(c);
            if used + w > room {
                break;
            }
            used += w;
            end += 1;
            if c == ' ' {
                if text {
                    brk = Some(end);
                }
            } else {
                text = true;
            }
        }
        if end < chars.len() {
            if let Some(b) = brk {
                end = b;
            }
            if end == start {
                end += 1;
            }
        }
        let mut line = Liner::new(prefix);
        for &(c, style) in &chars[start..end] {
            line.push(c, style);
        }
        lines.push(line.finish());
        if end >= chars.len() {
            return lines;
        }
        start = end;
        prefix = rest;
    }
}

/// `line` on a `bg` background padded to `width` cells.
pub fn on_bg(line: Line<'static>, width: usize, bg: Color) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|mut s| {
            if s.style.bg.is_none() {
                s.style = s.style.bg(bg);
            }
            s
        })
        .collect();
    let used = spans_width(&spans);
    if used < width {
        spans.push(Span::styled(
            " ".repeat(width - used),
            Style::default().bg(bg),
        ));
    }
    Line::from(spans)
}

/// `n` spaces as an unstyled span.
pub fn pad(n: usize) -> Span<'static> {
    Span::raw(" ".repeat(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn texts(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(text).collect()
    }

    #[test]
    fn clipping_counts_cells_and_marks_the_cut() {
        assert_eq!(clip("short", 5), "short");
        assert_eq!(clip("abcdefgh", 5), "abcd\u{2026}");
        assert_eq!(clip("\u{65e5}\u{672c}\u{8a9e}", 4), "\u{65e5}\u{2026}");
        assert_eq!(clip("abc", 0), "");
    }

    #[test]
    fn sanitizing_drops_escapes_and_controls_but_keeps_newlines() {
        assert_eq!(sanitize("plain\ntext"), "plain\ntext");
        assert_eq!(sanitize("\u{1b}[1;32mok\u{1b}[0m\r\n"), "ok\n");
        assert_eq!(sanitize("a\tb"), "a    b");
        assert_eq!(sanitize("\u{1b}]0;title\u{7}x"), "x");
        assert_eq!(sanitize("bell\u{7}"), "bell");
    }

    #[test]
    fn words_wrap_collapse_spaces_and_hang_under_the_prefix() {
        let runs = [(Style::default(), "one two  three four")];
        let lines = words(&runs, 10, &[Span::raw("- ")], &[pad(2)]);
        assert_eq!(texts(&lines), vec!["- one two", "  three", "  four"]);
    }

    #[test]
    fn words_hard_break_a_word_longer_than_the_line() {
        let runs = [(Style::default(), "abcdefghij klm")];
        assert_eq!(
            texts(&words(&runs, 4, &[], &[])),
            vec!["abcd", "efgh", "ij", "klm"]
        );
    }

    #[test]
    fn words_keep_styles_across_a_word() {
        let red = Style::default().fg(Color::Red);
        let runs = [
            (Style::default(), "see "),
            (red, "bold"),
            (Style::default(), "ly now"),
        ];
        let lines = words(&runs, 40, &[], &[]);
        assert_eq!(texts(&lines), vec!["see boldly now"]);
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|s| s.content == "bold" && s.style == red)
        );
    }

    #[test]
    fn mono_keeps_spacing_and_breaks_after_a_space() {
        let runs = [(Style::default(), "Name    Size  Health")];
        assert_eq!(
            texts(&mono(&runs, 40, &[], &[])),
            vec!["Name    Size  Health"]
        );
        assert_eq!(
            texts(&mono(&runs, 14, &[], &[])),
            vec!["Name    Size  ", "Health"]
        );
        let long = [(Style::default(), "abcdefghij")];
        assert_eq!(
            texts(&mono(&long, 4, &[], &[pad(1)])),
            vec!["abcd", " efg", " hij"]
        );
        assert_eq!(
            texts(&mono(&[(Style::default(), "")], 4, &[], &[])),
            vec![""]
        );
    }

    #[test]
    fn mono_does_not_break_inside_leading_indentation() {
        let runs = [(Style::default(), "    abcdefgh")];
        assert_eq!(texts(&mono(&runs, 8, &[], &[])), vec!["    abcd", "efgh"]);
    }

    #[test]
    fn a_background_line_is_padded_to_the_width() {
        let line = on_bg(Line::from(vec![Span::raw("ab")]), 5, Color::Blue);
        assert_eq!(text(&line), "ab   ");
        assert!(line.spans.iter().all(|s| s.style.bg == Some(Color::Blue)));
    }
}

//! Renders `agent_event` rows as terminal lines in the manner of the codex TUI:
//! a gutter mark per speaker, dim reasoning, one line per tool call, and a
//! small markdown subset for the agent's prose.

use database::schema::AgentEvent;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::terminal_mode::styling::{glyphs, THEME};

/// Longest tool result preview kept on one line.
const PREVIEW_CHARS: usize = 120;

pub fn render(events: &[AgentEvent], width: usize, show_reasoning: bool, spinner: &str) -> Vec<Line<'static>> {
    let width = width.max(8);
    let mut out: Vec<Line<'static>> = Vec::new();
    for ev in events {
        match ev.kind.as_str() {
            "turn_started" => {
                let label = ev.turn_id.clone().unwrap_or_else(|| "turn".into());
                let tail = width.saturating_sub(label.chars().count() + 5);
                out.push(Line::from(Span::styled(format!("{} {label} {}", rule(3), rule(tail)), muted())));
            }
            "turn_completed" => out.push(Line::from("")),
            "user" => {
                out.push(header(format!("{} You", glyphs::GUTTER), THEME.accent));
                push_wrapped(&mut out, &ev.text, width, Style::default().fg(THEME.text));
                out.push(Line::from(""));
            }
            "agent" => {
                let title = if ev.done {
                    format!("{} Agent", glyphs::GUTTER)
                } else {
                    format!("{} Agent {spinner}", glyphs::GUTTER)
                };
                out.push(header(title, THEME.success));
                out.extend(markdown(&ev.text, width));
                out.push(Line::from(""));
            }
            "reasoning" => {
                let text = ev.text.trim();
                if text.is_empty() {
                    continue;
                }
                let style = muted().add_modifier(Modifier::ITALIC);
                if show_reasoning {
                    out.push(Line::from(Span::styled("\u{00b7} thinking", style)));
                    push_wrapped(&mut out, text, width, muted());
                    out.push(Line::from(""));
                } else {
                    let first = text.lines().next().unwrap_or("");
                    out.push(Line::from(Span::styled(clip(&format!("\u{00b7} thinking: {first}"), width), style)));
                }
            }
            "tool_call" => {
                let failed = ev.item.as_ref().and_then(|i| i.get("error")).is_some_and(|e| !e.is_null());
                let (mark, color) = if failed {
                    ("!", THEME.error)
                } else if ev.done {
                    (glyphs::Glyph::Ok.as_str(), THEME.tertiary)
                } else {
                    (glyphs::AGENT_RUNNING, THEME.tertiary)
                };
                let (head, body) = ev.text.split_once('\n').unwrap_or((ev.text.as_str(), ""));
                out.push(Line::from(Span::styled(clip(&format!("{mark} {head}"), width), Style::default().fg(color))));
                let preview = body.trim().replace('\n', " ");
                if !preview.is_empty() {
                    let max = width.saturating_sub(2).min(PREVIEW_CHARS);
                    out.push(Line::from(Span::styled(format!("  {}", clip(&preview, max)), muted())));
                }
            }
            "command" => push_wrapped(&mut out, &ev.text, width, muted()),
            "approval" => {
                let text = format!("{} {}", glyphs::Glyph::Warning.as_str(), ev.text);
                push_wrapped(&mut out, &text, width, Style::default().fg(THEME.warning));
            }
            "error" => push_wrapped(&mut out, &format!("! {}", ev.text), width, Style::default().fg(THEME.error)),
            _ => {
                if !ev.text.trim().is_empty() {
                    push_wrapped(&mut out, &ev.text, width, muted());
                }
            }
        }
    }
    out
}

/// Headings, bullets, fenced code and inline bold/code; everything else wraps as prose.
pub fn markdown(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            for w in wrap(line, width.saturating_sub(2)) {
                out.push(Line::from(Span::styled(format!("  {w}"), Style::default().fg(THEME.tertiary))));
            }
            continue;
        }
        if line.trim().is_empty() {
            out.push(Line::from(""));
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let title = rest.trim_start_matches('#').trim();
            for w in wrap(title, width) {
                out.push(Line::from(Span::styled(w, Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD))));
            }
            continue;
        }
        let (prefix, body) = bullet(trimmed);
        let indent = " ".repeat(prefix.chars().count());
        for (i, w) in wrap(body, width.saturating_sub(prefix.chars().count())).into_iter().enumerate() {
            let lead = if i == 0 { prefix.clone() } else { indent.clone() };
            let mut spans = vec![Span::styled(lead, Style::default().fg(THEME.tertiary))];
            spans.extend(inline(&w));
            out.push(Line::from(spans));
        }
    }
    out
}

/// Splits a list marker (`- `, `* `, `1. `, `1) `) off a line.
fn bullet(line: &str) -> (String, &str) {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("\u{2022} "))
    {
        return (format!("{} ", glyphs::BULLET), rest);
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=3).contains(&digits) {
        let rest = &line[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return (format!("{} ", &line[..digits + 1]), r);
        }
    }
    (String::new(), line)
}

/// `**bold**` and `` `code` `` spans; an unclosed marker renders as plain text.
fn inline(text: &str) -> Vec<Span<'static>> {
    let base = Style::default().fg(THEME.text);
    let mut spans = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next = match (rest.find("**"), rest.find('`')) {
            (Some(b), Some(c)) => Some(if b <= c { (b, true) } else { (c, false) }),
            (Some(b), None) => Some((b, true)),
            (None, Some(c)) => Some((c, false)),
            (None, None) => None,
        };
        let Some((at, is_bold)) = next else {
            spans.push(Span::styled(rest.to_string(), base));
            break;
        };
        let (marker, style) = if is_bold {
            ("**", base.add_modifier(Modifier::BOLD))
        } else {
            ("`", Style::default().fg(THEME.tertiary))
        };
        let after = &rest[at + marker.len()..];
        let Some(end) = after.find(marker) else {
            spans.push(Span::styled(rest.to_string(), base));
            break;
        };
        if at > 0 {
            spans.push(Span::styled(rest[..at].to_string(), base));
        }
        spans.push(Span::styled(after[..end].to_string(), style));
        rest = &after[end + marker.len()..];
    }
    spans
}

fn header(title: String, color: Color) -> Line<'static> {
    Line::from(Span::styled(title, Style::default().fg(color).add_modifier(Modifier::BOLD)))
}

fn muted() -> Style {
    Style::default().fg(THEME.text_muted)
}

fn rule(n: usize) -> String {
    "\u{2500}".repeat(n)
}

fn push_wrapped(out: &mut Vec<Line<'static>>, text: &str, width: usize, style: Style) {
    for w in wrap(text, width) {
        out.push(Line::from(Span::styled(w, style)));
    }
}

/// Cuts to `max` columns with a trailing ellipsis.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}

/// Word-wraps to `width` columns with hard breaks for overlong words.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    fn push_word(out: &mut Vec<String>, line: &mut String, width: usize, word: &str) {
        let wlen = word.chars().count();
        let llen = line.chars().count();
        if llen > 0 && llen + 1 + wlen <= width {
            line.push(' ');
            line.push_str(word);
            return;
        }
        if llen > 0 {
            out.push(std::mem::take(line));
        }
        let mut chars = word.chars().peekable();
        while chars.peek().is_some() {
            let chunk: String = chars.by_ref().take(width).collect();
            if chars.peek().is_some() {
                out.push(chunk);
            } else {
                *line = chunk;
            }
        }
    }

    let width = width.max(1);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in raw.split_whitespace() {
            push_word(&mut out, &mut line, width, word);
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

/// Text of the first question's option `n` (1-based), when the agent offered options.
pub fn option_label(questions: Option<&Value>, n: usize) -> Option<String> {
    let q = questions?.as_array()?.first()?;
    let opt = q.get("options")?.as_array()?.get(n.checked_sub(1)?)?;
    opt.get("label").and_then(Value::as_str).map(str::to_string)
}

/// `{qid: [answer]}` for the first question, the shape the broker forwards to codex.
pub fn question_answers(questions: Option<&Value>, answer: &str) -> Value {
    let qid = questions
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|q| q.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("answer");
    serde_json::json!({ qid: [answer] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_marks_bullets_headings_and_code() {
        let lines = markdown("# Title\n- one **two** `three`\n```\ncode\n```", 40);
        let texts: Vec<String> = lines.iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect();
        assert_eq!(texts[0], "Title");
        assert!(texts[1].starts_with(glyphs::BULLET));
        assert!(texts[1].ends_with("one two three"));
        assert_eq!(texts[2], "  code");
    }

    #[test]
    fn unclosed_markers_stay_literal() {
        let spans = inline("a **b `c");
        let joined: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(joined, "a **b `c");
    }

    #[test]
    fn wrap_breaks_long_words() {
        assert_eq!(wrap("abcdefghij klm", 4), vec!["abcd", "efgh", "ij", "klm"]);
    }

    #[test]
    fn answers_take_the_first_question_id() {
        let q = serde_json::json!([{ "id": "q7", "options": [{ "label": "Yes" }, { "label": "No" }] }]);
        assert_eq!(option_label(Some(&q), 2).as_deref(), Some("No"));
        assert_eq!(question_answers(Some(&q), "Yes"), serde_json::json!({ "q7": ["Yes"] }));
        assert_eq!(question_answers(None, "x"), serde_json::json!({ "answer": ["x"] }));
    }
}

//! JSON values as indented terminal lines, coloured by kind.

use displays::ui_tools::chat_bubble::is_command_key;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use super::syntax::{self, Lang, Tok};
use super::wrap;
use crate::terminal_mode::styling::{THEME, glyphs};

/// Columns each nesting level is inset by.
const INDENT: usize = 2;
/// Pretty-printed lines shown before the rest is only counted.
const MAX_LINES: usize = 400;

type Runs = Vec<(Style, String)>;

/// A pretty-printed line before wrapping.
struct Pretty {
    indent: usize,
    runs: Runs,
}

/// `v` pretty-printed and wrapped to `width` cells; continuation lines hang one level deeper.
pub fn lines(v: &Value, width: usize) -> Vec<Line<'static>> {
    let mut pretty = Vec::new();
    node(&mut pretty, None, v, 0, false, width);
    let total = pretty.len();
    let mut out: Vec<Line<'static>> = pretty
        .into_iter()
        .take(MAX_LINES)
        .flat_map(|p| {
            wrap::mono(
                &p.runs,
                width,
                &[wrap::pad(p.indent)],
                &[wrap::pad(p.indent + INDENT)],
            )
        })
        .collect();
    if total > MAX_LINES {
        let note = format!("{} {} more lines", glyphs::ELLIPSIS, total - MAX_LINES);
        out.push(Line::from(Span::styled(note, Tok::Punct.style())));
    }
    out
}

fn node(
    out: &mut Vec<Pretty>,
    key: Option<&str>,
    v: &Value,
    indent: usize,
    comma: bool,
    width: usize,
) {
    let mut head: Runs = Vec::new();
    if let Some(k) = key {
        head.push((Tok::Key.style(), format!("\"{}\"", wrap::sanitize(k))));
        head.push((Tok::Punct.style(), ": ".into()));
    }
    let tail = if comma { "," } else { "" };
    match v {
        Value::Object(map) if map.is_empty() => {
            head.push((Tok::Punct.style(), format!("{{}}{tail}")));
            out.push(Pretty { indent, runs: head });
        }
        Value::Array(items) if items.is_empty() => {
            head.push((Tok::Punct.style(), format!("[]{tail}")));
            out.push(Pretty { indent, runs: head });
        }
        Value::Object(map) => {
            head.push((Tok::Punct.style(), "{".into()));
            out.push(Pretty { indent, runs: head });
            let last = map.len() - 1;
            for (i, (k, child)) in map.iter().enumerate() {
                node(out, Some(k), child, indent + INDENT, i < last, width);
            }
            out.push(Pretty {
                indent,
                runs: vec![(Tok::Punct.style(), format!("}}{tail}"))],
            });
        }
        Value::Array(items) => {
            if items
                .iter()
                .all(|i| !i.is_object() && !i.is_array() && !is_block(i))
            {
                let mut inline = inline_array(items);
                inline.push((Tok::Punct.style(), tail.into()));
                if indent + cells(&head) + cells(&inline) <= width {
                    head.extend(inline);
                    out.push(Pretty { indent, runs: head });
                    return;
                }
            }
            head.push((Tok::Punct.style(), "[".into()));
            out.push(Pretty { indent, runs: head });
            let last = items.len() - 1;
            for (i, child) in items.iter().enumerate() {
                node(out, None, child, indent + INDENT, i < last, width);
            }
            out.push(Pretty {
                indent,
                runs: vec![(Tok::Punct.style(), format!("]{tail}"))],
            });
        }
        Value::String(s) if is_block(v) => {
            let inset = if head.is_empty() {
                indent
            } else {
                indent + INDENT
            };
            if !head.is_empty() {
                head.pop();
                head.push((Tok::Punct.style(), ":".into()));
                out.push(Pretty { indent, runs: head });
            }
            let lang = key
                .filter(|k| is_command_key(k))
                .map(|_| Lang::guess_shell(s));
            let text = wrap::sanitize(s);
            for line in syntax::styled_lines(lang, text.trim_end_matches('\n')) {
                let runs = line
                    .into_iter()
                    .map(|(style, part)| {
                        (
                            if lang.is_some() {
                                style
                            } else {
                                Tok::Str.style()
                            },
                            part.to_string(),
                        )
                    })
                    .collect();
                out.push(Pretty {
                    indent: inset,
                    runs,
                });
            }
        }
        scalar => {
            head.extend(scalar_runs(key, scalar));
            head.push((Tok::Punct.style(), tail.into()));
            out.push(Pretty { indent, runs: head });
        }
    }
}

/// True for a string shown as a block under its key.
fn is_block(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.trim_end_matches('\n').contains('\n'))
}

fn inline_array(items: &[Value]) -> Runs {
    let mut runs = vec![(Tok::Punct.style(), "[".to_string())];
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            runs.push((Tok::Punct.style(), ", ".into()));
        }
        runs.extend(scalar_runs(None, item));
    }
    runs.push((Tok::Punct.style(), "]".into()));
    runs
}

fn scalar_runs(key: Option<&str>, v: &Value) -> Runs {
    match v {
        Value::String(s) => {
            let text = wrap::sanitize(s);
            let quote = (Tok::Str.style(), "\"".to_string());
            if key.is_some_and(is_command_key) {
                let mut runs = vec![quote.clone()];
                runs.extend(
                    syntax::tokens(Lang::guess_shell(&text), &text)
                        .into_iter()
                        .map(|(t, part)| (t.style(), part.to_string())),
                );
                runs.push(quote);
                runs
            } else {
                vec![(Tok::Str.style(), format!("\"{text}\""))]
            }
        }
        Value::Number(n) => vec![(Tok::Num.style(), n.to_string())],
        Value::Bool(b) => vec![(Tok::Literal.style(), b.to_string())],
        _ => vec![(null_style(), "null".to_string())],
    }
}

fn null_style() -> Style {
    Style::default()
        .fg(THEME.overlay)
        .add_modifier(Modifier::ITALIC)
}

fn cells(runs: &Runs) -> usize {
    runs.iter().map(|(_, s)| wrap::width(s)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn texts(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn style_of(lines: &[Line<'_>], text: &str) -> Option<Style> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == text)
            .map(|s| s.style)
    }

    #[test]
    fn objects_indent_and_close_with_commas() {
        let v =
            json!({"a_host": "PC-1", "b_disks": [{"a_name": "ssd", "b_ok": true}], "c_note": null});
        assert_eq!(
            texts(&lines(&v, 80)),
            vec![
                "{",
                "  \"a_host\": \"PC-1\",",
                "  \"b_disks\": [",
                "    {",
                "      \"a_name\": \"ssd\",",
                "      \"b_ok\": true",
                "    }",
                "  ],",
                "  \"c_note\": null",
                "}",
            ]
        );
    }

    #[test]
    fn keys_strings_numbers_bools_and_null_get_their_own_colours() {
        let v = json!({"k": "s", "n": 5, "b": false, "z": null});
        let out = lines(&v, 80);
        let styles = ["\"k\"", "\"s\"", "5", "false", "null"]
            .map(|t| style_of(&out, t).unwrap_or_else(|| panic!("no span {t}")));
        for (i, a) in styles.iter().enumerate() {
            for b in &styles[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn short_scalar_arrays_stay_on_one_line() {
        assert_eq!(
            texts(&lines(&json!({"ids": [1, 2, 3]}), 80)),
            vec!["{", "  \"ids\": [1, 2, 3]", "}"]
        );
        let long = json!({"ids": [100000, 200000, 300000]});
        assert_eq!(texts(&lines(&long, 20))[1], "  \"ids\": [");
        assert_eq!(texts(&lines(&json!([]), 20)), vec!["[]"]);
        assert_eq!(texts(&lines(&json!({}), 20)), vec!["{}"]);
    }

    #[test]
    fn multiline_strings_open_as_a_block_under_their_key() {
        let v = json!({"script": "Get-PhysicalDisk |\n  Select-Object Health\n"});
        let out = lines(&v, 80);
        assert_eq!(
            texts(&out),
            vec![
                "{",
                "  \"script\":",
                "    Get-PhysicalDisk |",
                "      Select-Object Health",
                "}"
            ]
        );
        assert_eq!(
            style_of(&out, "Get-PhysicalDisk").map(|s| s.fg),
            Some(Tok::Func.style().fg)
        );
    }

    #[test]
    fn command_strings_are_coloured_inline() {
        let out = lines(&json!({"command": "ls -la"}), 80);
        assert_eq!(texts(&out)[1], "  \"command\": \"ls -la\"");
        assert_eq!(style_of(&out, "-la"), Some(Tok::Var.style()));
    }

    #[test]
    fn long_values_wrap_under_their_line() {
        let v = json!({"note": "alpha beta gamma delta"});
        let out = texts(&lines(&v, 22));
        assert_eq!(out[1], "  \"note\": \"alpha beta ");
        assert_eq!(out[2], "    gamma delta\"");
        assert!(out.iter().all(|l| wrap::width(l) <= 22), "{out:?}");
    }

    #[test]
    fn huge_values_stop_with_a_count() {
        let v = Value::Array((0..1000).map(|i| json!({ "i": i })).collect());
        let out = texts(&lines(&v, 80));
        assert_eq!(out.len(), MAX_LINES + 1);
        assert!(
            out[MAX_LINES].ends_with("more lines"),
            "{:?}",
            out[MAX_LINES]
        );
    }
}

//! Markdown renderer for assistant chat output.
//!
//! EasyMark treats `_` as italic, which corrupts every `snake_case` identifier
//! an assistant writes. This handles the subset LLMs actually emit — fenced
//! code, headings, bullets, `**bold**`, `` `code` `` — and leaves `_` literal.

use eframe::egui::{Color32, Frame, Label, RichText, TextWrapMode, Ui};

/// Renders `text` as chat markdown into `ui`.
pub fn render(ui: &mut Ui, text: &str) {
    let mut in_code = false;
    let mut lang = String::new();
    let mut code = String::new();

    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            if in_code {
                code_block(ui, &lang, &code);
                code.clear();
                lang.clear();
                in_code = false;
            } else {
                in_code = true;
                lang = rest.trim().to_string();
            }
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        line_ui(ui, line);
    }
    // Unterminated fence while a response is still streaming.
    if in_code && !code.is_empty() {
        code_block(ui, &lang, &code);
    }
}

fn code_block(ui: &mut Ui, lang: &str, code: &str) {
    Frame::group(ui.style())
        .fill(ui.visuals().extreme_bg_color)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if !lang.is_empty() {
                    ui.label(RichText::new(lang).small().weak());
                }
                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    if ui.small_button(crate::ui_tools::icons::COPY).on_hover_text("Copy").clicked() {
                        ui.ctx().copy_text(code.to_string());
                    }
                });
            });
            ui.add(
                Label::new(RichText::new(code.trim_end()).monospace())
                    .wrap_mode(TextWrapMode::Extend),
            );
        });
}

fn line_ui(ui: &mut Ui, line: &str) {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        ui.add_space(4.0);
        return;
    }
    // Headings.
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes > 0 && hashes <= 6 && trimmed.chars().nth(hashes) == Some(' ') {
        let size = match hashes {
            1 => 20.0,
            2 => 17.0,
            _ => 15.0,
        };
        ui.add_space(3.0);
        inline_ui(ui, trimmed[hashes + 1..].trim(), Some(size));
        return;
    }
    // Bullets and numbered items keep their marker and indent.
    let bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
    let numbered = trimmed
        .split_once(". ")
        .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if bullet || numbered {
        let indent = (line.len() - trimmed.len()) as f32 * 3.0;
        ui.horizontal_wrapped(|ui| {
            ui.add_space(12.0 + indent);
            let (marker, rest) = if bullet {
                ("\u{2022}".to_string(), &trimmed[2..])
            } else {
                let (n, r) = trimmed.split_once(". ").unwrap_or(("", trimmed));
                (format!("{n}."), r)
            };
            ui.label(RichText::new(marker).weak());
            inline_ui(ui, rest, None);
        });
        return;
    }
    inline_ui(ui, trimmed, None);
}

/// Emits one line, honoring `**bold**` and `` `code` `` only. `_` stays literal.
fn inline_ui(ui: &mut Ui, text: &str, heading: Option<f32>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut rest = text;
        while !rest.is_empty() {
            let bold = rest.find("**");
            let code = rest.find('`');
            let next = match (bold, code) {
                (Some(b), Some(c)) => Some(b.min(c)),
                (Some(b), None) => Some(b),
                (None, Some(c)) => Some(c),
                (None, None) => None,
            };
            let Some(at) = next else {
                push(ui, rest, heading, false, false);
                break;
            };
            if at > 0 {
                push(ui, &rest[..at], heading, false, false);
            }
            let is_bold = rest[at..].starts_with("**");
            let (open, close) = if is_bold { (2usize, "**") } else { (1usize, "`") };
            let after = &rest[at + open..];
            match after.find(close) {
                Some(end) => {
                    push(ui, &after[..end], heading, is_bold, !is_bold);
                    rest = &after[end + open..];
                }
                // Unclosed marker mid-stream: print the remainder verbatim.
                None => {
                    push(ui, &rest[at..], heading, false, false);
                    break;
                }
            }
        }
    });
}

fn push(ui: &mut Ui, s: &str, heading: Option<f32>, bold: bool, mono: bool) {
    if s.is_empty() {
        return;
    }
    let mut rt = RichText::new(s);
    if let Some(size) = heading {
        rt = rt.size(size).strong();
    }
    if bold {
        rt = rt.strong();
    }
    if mono {
        rt = rt.monospace().color(Color32::LIGHT_GREEN);
    }
    ui.label(rt);
}

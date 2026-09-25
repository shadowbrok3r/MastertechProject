//! Collapsible JSON tree: a framed toggle and copy button per container, leaves as wrapped text.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Id, Layout, RichText, Stroke, TextFormat, Ui,
    text::LayoutJob,
};
use serde_json::Value;

use super::{ChatStyle, pane, shell, tinted, wrap_width};
use crate::ui_tools::icons;

/// Pixels each nesting level is inset by.
const INDENT: f32 = 12.0;
/// Longest string shown inline with its key.
const INLINE: usize = 60;
/// Width of a header row's copy button.
const COPY_W: f32 = 30.0;
const ROW_H: f32 = 20.0;

/// What a node is called in its header or on its line.
#[derive(Clone, Copy)]
enum Label<'a> {
    Root,
    Key(&'a str),
    Index(usize),
}

/// Draws `v` under `id`; the root starts open and everything below it closed.
pub(crate) fn show(ui: &mut Ui, id: Id, v: &Value, style: &ChatStyle) {
    node(ui, id, Label::Root, v, 0, style);
}

fn node(ui: &mut Ui, id: Id, label: Label<'_>, v: &Value, depth: usize, style: &ChatStyle) {
    match v {
        Value::Object(_) | Value::Array(_) => container(ui, id, label, v, depth, style),
        _ => leaf(ui, label, v, style),
    }
}

fn container(ui: &mut Ui, id: Id, label: Label<'_>, v: &Value, depth: usize, style: &ChatStyle) {
    let open_id = id.with("open");
    let mut open = ui.data_mut(|d| *d.get_temp_mut_or(open_id, depth == 0));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (ui.available_width() - COPY_W).max(48.0);
        let text = RichText::new(header(&label, v, open))
            .font(style.mono.clone())
            .color(style.tertiary);
        let clicked = tinted(ui, style.primary_hue, |ui| {
            ui.add_sized(
                [w, ROW_H],
                egui::Button::new((text, egui::Atom::grow()))
                    .wrap_mode(egui::TextWrapMode::Truncate),
            )
        })
        .clicked();
        if clicked {
            open = !open;
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let copy = egui::Button::new(RichText::new(icons::COPY).font(style.small.clone()))
                .fill(pane(style.primary_hue, 0.06))
                .stroke(Stroke::new(1.0, style.rim))
                .corner_radius(CornerRadius::same(5));
            if ui
                .add_sized([COPY_W - 4.0, ROW_H], copy)
                .on_hover_text("Copy this JSON")
                .clicked()
            {
                ui.ctx()
                    .copy_text(serde_json::to_string_pretty(v).unwrap_or_default());
            }
        });
    });
    if !open {
        return;
    }
    let inner = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(INDENT);
        ui.vertical(|ui| match v {
            Value::Object(o) => {
                for (k, c) in o {
                    node(ui, id.with(("k", k)), Label::Key(k), c, depth + 1, style);
                }
            }
            Value::Array(a) => {
                for (i, c) in a.iter().enumerate() {
                    node(ui, id.with(("i", i)), Label::Index(i), c, depth + 1, style);
                }
            }
            _ => {}
        });
    });
    let r = inner.response.rect;
    ui.painter().vline(
        r.left() + INDENT * 0.5,
        r.y_range(),
        Stroke::new(1.0, style.rim_bright),
    );
}

/// The text of a container's header button.
fn header(label: &Label<'_>, v: &Value, open: bool) -> String {
    let marker = if open {
        icons::CHEV_OPEN
    } else {
        icons::CHEV_CLOSED
    };
    let braces = match v {
        Value::Object(o) if o.is_empty() => "{}".to_string(),
        Value::Array(a) if a.is_empty() => "[]".to_string(),
        Value::Object(o) => format!("{{{}}}", o.len()),
        Value::Array(a) => format!("[{}]", a.len()),
        _ => String::new(),
    };
    match label {
        Label::Root => format!("{marker} {braces}"),
        Label::Key(k) => format!("{marker} \"{k}\": {braces}"),
        Label::Index(i) => format!("{marker} [{i}]: {braces}"),
    }
}

fn leaf(ui: &mut Ui, label: Label<'_>, v: &Value, style: &ChatStyle) {
    let (text, color, quoted) = match v {
        Value::String(s) => (s.clone(), style.text, true),
        Value::Number(n) => (n.to_string(), style.tertiary, false),
        Value::Bool(b) => (b.to_string(), style.secondary, false),
        _ => ("null".to_string(), style.primary, false),
    };
    let shell = quoted && matches!(label, Label::Key(k) if shell::is_command_key(k));
    let font = &style.mono;
    let fmt = |c: Color32| TextFormat {
        font_id: font.clone(),
        color: c,
        ..Default::default()
    };
    let w = wrap_width(ui);
    let mut job = LayoutJob::default();
    job.wrap.max_width = w;
    match label {
        Label::Key(k) => {
            job.append(&format!("\"{k}\""), 0.0, fmt(style.tertiary));
            job.append(": ", 0.0, fmt(style.primary));
        }
        Label::Index(i) => {
            job.append(&format!("[{i}]"), 0.0, fmt(style.primary));
            job.append(": ", 0.0, fmt(style.primary));
        }
        Label::Root => {}
    }
    if inline_value(v) {
        if shell {
            job.append("\"", 0.0, fmt(style.primary));
            shell::append(&mut job, &text, font, &style.shell_colors(color));
            job.append("\"", 0.0, fmt(style.primary));
        } else {
            let shown = if quoted { format!("\"{text}\"") } else { text };
            job.append(&shown, 0.0, fmt(color));
        }
        let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
        ui.add(egui::Label::new(galley));
        return;
    }
    if !job.text.is_empty() {
        let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
        ui.add(egui::Label::new(galley));
    }
    let mut body = LayoutJob::default();
    body.wrap.max_width = (w - INDENT).max(48.0);
    if shell {
        shell::append(&mut body, &text, font, &style.shell_colors(color));
    } else {
        body.append(&text, 0.0, fmt(color));
    }
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(body));
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(INDENT);
        ui.add(egui::Label::new(galley));
    });
}

/// True when a value is shown inline with its key.
fn inline_value(v: &Value) -> bool {
    match v {
        Value::String(s) => s.chars().count() <= INLINE && !s.contains('\n'),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn short_values_share_the_line_and_long_or_multiline_ones_go_below() {
        assert!(inline_value(&json!("ls -la")));
        assert!(inline_value(&json!(42)));
        assert!(inline_value(&json!(null)));
        assert!(!inline_value(&json!("a".repeat(INLINE + 1))));
        assert!(!inline_value(&json!("two\nlines")));
    }

    #[test]
    fn headers_name_the_node_and_count_its_children() {
        let v = json!({"a": 1, "b": 2, "c": 3});
        assert_eq!(
            header(&Label::Root, &v, false),
            format!("{} {{3}}", icons::CHEV_CLOSED)
        );
        assert_eq!(
            header(&Label::Key("args"), &v, true),
            format!("{} \"args\": {{3}}", icons::CHEV_OPEN)
        );
        assert_eq!(
            header(&Label::Index(2), &json!([1]), false),
            format!("{} [2]: [1]", icons::CHEV_CLOSED)
        );
        assert_eq!(
            header(&Label::Root, &json!({}), true),
            format!("{} {{}}", icons::CHEV_OPEN)
        );
        assert_eq!(
            header(&Label::Root, &json!([]), true),
            format!("{} []", icons::CHEV_OPEN)
        );
    }
}

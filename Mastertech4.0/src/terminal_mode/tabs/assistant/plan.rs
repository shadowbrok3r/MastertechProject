//! The agent's current plan over the composer: one summary line folded, every step unfolded.

use std::cell::Cell;
use std::collections::HashMap;

use database::schema::{Plan, PlanStatus};
use ratatui::{
    Frame,
    crossterm::event::{MouseButton, MouseEvent, MouseEventKind},
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use super::{transcript, wrap};
use crate::terminal_mode::styling::{THEME, glyphs};

/// Cells of the progress bar.
const BAR_CELLS: usize = 12;
/// Fewest step lines an unfolded panel keeps room for.
const MIN_BODY: u16 = 3;
/// Lines a wheel notch scrolls the steps.
const WHEEL_STEP: usize = 2;

#[derive(Default)]
pub struct PlanPanel {
    /// The technician's fold choice; `None` unfolds a plan with steps left and folds a finished one.
    open: Cell<Option<bool>>,
    /// Lines scrolled past at the top of the unfolded steps.
    scroll: Cell<usize>,
    /// Brings the step in progress into view at the next redraw.
    follow: Cell<bool>,
    /// The toggle row and the step lines at the last redraw.
    head: Cell<Rect>,
    body: Cell<Rect>,
    /// Lines the steps took at the last redraw.
    lines: Cell<usize>,
    /// Status of each step at the last look, by step text.
    seen: HashMap<String, PlanStatus>,
    /// The plan update looked at last.
    seen_key: Option<String>,
}

impl PlanPanel {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_open(&self, plan: &Plan) -> bool {
        self.open.get().unwrap_or(!plan.complete())
    }

    pub fn toggle(&self, plan: &Plan) {
        self.open.set(Some(!self.is_open(plan)));
        self.follow.set(true);
    }

    /// Looks at plan update `key`: whether it is the session's first plan, and the steps that turned done since the last.
    pub fn observe(&mut self, key: &str, plan: &Plan) -> (bool, Vec<usize>) {
        if self.seen_key.as_deref() == Some(key) {
            return (false, Vec::new());
        }
        let first = self.seen_key.is_none();
        let done = plan
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.status == PlanStatus::Completed
                    && self
                        .seen
                        .get(&s.step)
                        .is_some_and(|was| *was != PlanStatus::Completed)
            })
            .map(|(i, _)| i)
            .collect();
        self.seen = plan
            .steps
            .iter()
            .map(|s| (s.step.clone(), s.status))
            .collect();
        self.seen_key = Some(key.to_string());
        self.follow.set(true);
        (first, done)
    }

    /// Rows the panel takes `width` cells wide with at most `max` rows to spare.
    pub fn height(&self, plan: &Plan, width: u16, max: u16) -> u16 {
        if !self.is_open(plan) {
            return 1;
        }
        let lines = body_lines(
            plan,
            usize::from(width.saturating_sub(2)),
            glyphs::SPINNER[0],
        )
        .0
        .len() as u16;
        (lines + 2).min(max.max(MIN_BODY + 2))
    }

    /// Draws the panel into `area`; returns where each step's lines are on screen.
    pub fn draw(&self, f: &mut Frame, area: Rect, plan: &Plan, spinner: &str) -> Vec<Option<Rect>> {
        let open = self.is_open(plan);
        if !open || area.height < 3 {
            self.head.set(Rect { height: 1, ..area });
            self.body.set(Rect::default());
            let line = summary(plan, usize::from(area.width), false, spinner);
            f.render_widget(
                Paragraph::new(line).style(Style::default().bg(THEME.bg)),
                Rect { height: 1, ..area },
            );
            return Vec::new();
        }
        let ink = if plan.complete() {
            THEME.success
        } else {
            THEME.tertiary
        };
        let title = summary(
            plan,
            usize::from(area.width.saturating_sub(18)),
            true,
            spinner,
        );
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ink))
            .title(title)
            .title(Line::styled(" Ctrl+P fold ", muted()).right_aligned())
            .style(Style::default().bg(THEME.bg));
        let inner = block.inner(area);
        let (lines, starts) = body_lines(plan, usize::from(inner.width), spinner);
        let height = usize::from(inner.height);
        let max_scroll = lines.len().saturating_sub(height);
        if self.follow.take() {
            let current = plan
                .steps
                .iter()
                .position(|s| s.status != PlanStatus::Completed)
                .unwrap_or(0);
            let at = starts.get(current).copied().unwrap_or(0);
            if at < self.scroll.get() || at >= self.scroll.get() + height {
                self.scroll.set(at.saturating_sub(1));
            }
        }
        let scroll = self.scroll.get().min(max_scroll);
        self.scroll.set(scroll);
        let below = lines.len().saturating_sub(scroll + height);
        if scroll > 0 || below > 0 {
            let text = format!(
                " {} {scroll} {} {} {below} ",
                glyphs::SCROLL_UP,
                glyphs::DOT,
                glyphs::SCROLL_DOWN
            );
            block = block.title_bottom(Line::styled(text, muted()).right_aligned());
        }
        self.head.set(Rect { height: 1, ..area });
        self.body.set(inner);
        self.lines.set(lines.len());
        let shown: Vec<Line<'static>> = lines.into_iter().skip(scroll).take(height).collect();
        f.render_widget(Paragraph::new(shown).block(block), area);

        let mut ends = starts.iter().skip(1).copied().collect::<Vec<_>>();
        ends.push(self.lines.get());
        starts
            .iter()
            .zip(ends)
            .map(|(&start, end)| {
                let top = start.max(scroll);
                let bottom = end.min(scroll + height);
                (top < bottom).then(|| Rect {
                    x: inner.x,
                    y: inner.y + (top - scroll) as u16,
                    width: inner.width,
                    height: (bottom - top) as u16,
                })
            })
            .collect()
    }

    /// A click on the summary row folds or unfolds; the wheel over the steps scrolls them. False when the event is elsewhere.
    pub fn handle_mouse(&self, mouse: &MouseEvent, plan: Option<&Plan>) -> bool {
        let Some(plan) = plan else { return false };
        let pos = Position::new(mouse.column, mouse.row);
        let body = self.body.get();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if self.head.get().contains(pos) => {
                self.toggle(plan);
                true
            }
            MouseEventKind::ScrollUp if body.contains(pos) => {
                self.scroll
                    .set(self.scroll.get().saturating_sub(WHEEL_STEP));
                true
            }
            MouseEventKind::ScrollDown if body.contains(pos) => {
                let max = self.lines.get().saturating_sub(usize::from(body.height));
                self.scroll.set((self.scroll.get() + WHEEL_STEP).min(max));
                true
            }
            _ => false,
        }
    }
}

fn muted() -> Style {
    Style::default().fg(THEME.text_muted)
}

/// The explanation and every step wrapped to `width`, with the line each step starts on.
fn body_lines(plan: &Plan, width: usize, spinner: &str) -> (Vec<Line<'static>>, Vec<usize>) {
    let mut lines = Vec::new();
    if let Some(why) = &plan.explanation {
        lines.extend(wrap::words(
            &[(muted(), why.as_str())],
            width.max(4),
            &[],
            &[],
        ));
    }
    let mut starts = Vec::with_capacity(plan.steps.len());
    for step in &plan.steps {
        starts.push(lines.len());
        lines.extend(transcript::plan_step_lines(step, width, spinner));
    }
    (lines, starts)
}

/// Fold marker, progress and, while folded, the step in progress, cut to `width`.
fn summary(plan: &Plan, width: usize, open: bool, spinner: &str) -> Line<'static> {
    let complete = plan.complete();
    let ink = if complete {
        THEME.success
    } else {
        THEME.tertiary
    };
    let mut spans = vec![
        Span::styled(
            if open {
                glyphs::ROW_OPEN
            } else {
                glyphs::ROW_CLOSED
            },
            Style::default().fg(ink),
        ),
        Span::styled(
            " Plan ",
            Style::default().fg(ink).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{}/{} ", plan.done(), plan.steps.len()), muted()),
    ];
    spans.extend(transcript::progress_bar(
        plan.done(),
        plan.steps.len(),
        BAR_CELLS,
        ink,
    ));
    if open {
        spans.push(Span::raw(" "));
        return wrap::fit(spans, width);
    }
    match plan.current() {
        Some(step) => {
            let (mark, mark_ink) = match step.status {
                PlanStatus::InProgress => (spinner, THEME.accent),
                _ => (glyphs::checkbox(false), THEME.text_muted),
            };
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                mark.to_string(),
                Style::default().fg(mark_ink),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                wrap::one_line(&step.step),
                Style::default().fg(THEME.text),
            ));
        }
        None => spans.push(Span::styled(
            format!("  {} All steps done", glyphs::checkbox(true)),
            Style::default().fg(THEME.success),
        )),
    }
    let hint = " Ctrl+P unfold";
    let used = wrap::spans_width(&spans);
    let room = width.saturating_sub(wrap::width(hint));
    if used <= room {
        spans.push(Span::raw(" ".repeat(room - used)));
        spans.push(Span::styled(hint, muted()));
        return Line::from(spans);
    }
    wrap::fit(spans, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(done: usize, total: usize) -> Plan {
        let steps: Vec<_> = (0..total)
            .map(|i| {
                let status = match i.cmp(&done) {
                    std::cmp::Ordering::Less => "completed",
                    std::cmp::Ordering::Equal => "inProgress",
                    std::cmp::Ordering::Greater => "pending",
                };
                json!({ "step": format!("step {i}"), "status": status })
            })
            .collect();
        Plan::from_value(&json!({ "plan": steps })).expect("plan")
    }

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_plan_with_steps_left_unfolds_and_a_finished_one_folds_until_toggled() {
        let panel = PlanPanel::default();
        let open = plan(2, 6);
        assert!(panel.is_open(&open));
        assert_eq!(panel.height(&open, 60, 40), 8);
        assert_eq!(
            panel.height(&open, 60, 4),
            MIN_BODY + 2,
            "capped, keeping room for a few steps"
        );
        let done = plan(3, 3);
        assert!(!panel.is_open(&done));
        assert_eq!(panel.height(&done, 60, 40), 1);
        panel.toggle(&done);
        assert!(panel.is_open(&done));
    }

    #[test]
    fn completions_are_reported_once_and_never_on_the_first_look() {
        let mut panel = PlanPanel::default();
        assert_eq!(panel.observe("p1", &plan(1, 4)), (true, vec![]));
        assert_eq!(
            panel.observe("p1", &plan(3, 4)),
            (false, vec![]),
            "the same update is looked at once"
        );
        assert_eq!(panel.observe("p2", &plan(3, 4)), (false, vec![1, 2]));
        assert_eq!(panel.observe("p3", &plan(3, 4)), (false, vec![]));
    }

    #[test]
    fn the_folded_line_names_the_step_in_progress_and_fits() {
        let line = summary(&plan(2, 5), 80, false, "\u{25d1}");
        let t = text(&line);
        assert!(t.starts_with("\u{25b8} Plan 2/5 "), "{t}");
        assert!(
            t.contains("\u{25d1} step 2") && t.ends_with("Ctrl+P unfold"),
            "{t}"
        );
        assert_eq!(wrap::width(&t), 80);
        for width in [10, 30] {
            assert!(wrap::width(&text(&summary(&plan(2, 5), width, false, "x"))) <= width);
        }
        assert!(text(&summary(&plan(5, 5), 80, false, "x")).contains("All steps done"));
    }
}

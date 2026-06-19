use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::styling::THEME;
use super::button::truncate_to_width;

/// A single-line selectable menu row: marker prefix, label, optional right-aligned hint.
/// Shared by `DropdownMenu` and other list-style menus.
#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    /// Current page / checked state — pink `✓` prefix.
    pub active: bool,
    /// Right-aligned hint (shortcut, status, count).
    pub hint: Option<String>,
    pub disabled: bool,
}

#[allow(dead_code)]
impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), active: false, hint: None, disabled: false }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Builds a styled line: `✓`/`▸`/`  ` marker, label, and a right-aligned hint.
    pub fn to_line(&self, highlighted: bool, width: u16) -> Line<'static> {
        let (marker, marker_color) = if self.active {
            ("\u{2713} ", THEME.accent)
        } else if highlighted {
            ("\u{25b8} ", THEME.tertiary)
        } else {
            ("  ", THEME.tertiary)
        };

        let label_color = if self.disabled {
            Color::DarkGray
        } else if highlighted {
            THEME.accent
        } else {
            THEME.text
        };
        let mut label_style = Style::new().fg(label_color);
        if highlighted || self.active {
            label_style = label_style.add_modifier(Modifier::BOLD);
        }

        let total = width as usize;
        let hint = self.hint.clone().unwrap_or_default();
        let hint_w = hint.width();
        let hint_cost = if hint_w > 0 { hint_w + 1 } else { 0 };
        let label_budget = total.saturating_sub(2 + hint_cost).max(1);
        let label = truncate_to_width(&self.label, label_budget);

        let used = 2 + label.width() + hint_w;
        let pad = total.saturating_sub(used);

        let mut spans = vec![
            Span::styled(marker.to_string(), Style::new().fg(marker_color)),
            Span::styled(label, label_style),
        ];
        if hint_w > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(hint, Style::new().fg(THEME.text_muted)));
        }
        Line::from(spans)
    }

    /// Display width of the row at its natural size (no truncation).
    pub fn natural_width(&self) -> usize {
        let hint_w = self.hint.as_ref().map(|h| h.width() + 2).unwrap_or(0);
        2 + self.label.width() + hint_w
    }
}

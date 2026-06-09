use ratatui::{
    layout::{Position, Rect},
    style::Style,
    widgets::{Block, BorderType, Clear, List, ListItem},
    Frame,
};

use crate::terminal_mode::styling::{APP_BACKGROUND, THEME};
use super::menu_item::MenuItem;

/// A hover/click dropdown overlay anchored under a trigger rect.
/// Pink-bordered panel of compact `MenuItem` rows. Owner drives open/close and
/// reads `selected()` / `on_click()`. The hover bridge (`bridge_contains`) keeps
/// the menu open while the cursor travels from trigger to panel.
#[derive(Clone, Debug, Default)]
pub struct DropdownMenu {
    title: String,
    items: Vec<MenuItem>,
    anchor: Rect,
    open: bool,
    selected: Option<usize>,
    rect: Option<Rect>,
}

#[allow(dead_code)]
impl DropdownMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn anchor(&self) -> Rect {
        self.anchor
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Open the menu under `anchor` with `items`. Resets highlight to the active row.
    pub fn open_at(&mut self, anchor: Rect, items: Vec<MenuItem>, title: impl Into<String>) {
        self.selected = items.iter().position(|i| i.active);
        self.items = items;
        self.anchor = anchor;
        self.title = title.into();
        self.open = true;
        self.rect = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.rect = None;
    }

    /// Computes the panel rect under the anchor, clamped to `frame`.
    fn layout(&self, frame: Rect) -> Rect {
        let content_w = self
            .items
            .iter()
            .map(|i| i.natural_width())
            .chain(std::iter::once(self.title.len() + 2))
            .max()
            .unwrap_or(12);
        let width = (content_w as u16 + 2).clamp(12, frame.width.max(12));
        let height = (self.items.len() as u16 + 2).min(frame.height.max(3));

        let x = self
            .anchor
            .x
            .min(frame.right().saturating_sub(width))
            .max(frame.x);

        // Prefer below the anchor; flip above if it would overflow the frame bottom.
        let below_y = self.anchor.bottom();
        let y = if below_y + height <= frame.bottom() {
            below_y
        } else {
            self.anchor.y.saturating_sub(height).max(frame.y)
        };

        Rect { x, y, width, height }
    }

    /// Renders the panel. No-op when closed.
    pub fn render(&mut self, f: &mut Frame, frame: Rect) {
        if !self.open || self.items.is_empty() {
            return;
        }
        let rect = self.layout(frame);
        self.rect = Some(rect);

        f.render_widget(Clear, rect);

        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(THEME.accent))
            .style(Style::new().bg(APP_BACKGROUND));
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let rows: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let selected = self.selected == Some(idx);
                let row = ListItem::new(item.to_line(selected, inner.width));
                if selected {
                    row.style(Style::new().bg(THEME.surface))
                } else {
                    row
                }
            })
            .collect();
        f.render_widget(List::new(rows), inner);
    }

    /// True if `pos` is inside the panel.
    pub fn rect_contains(&self, pos: Position) -> bool {
        self.rect.map_or(false, |r| r.contains(pos))
    }

    /// True if `pos` is inside the anchor OR the panel (hover bridge).
    pub fn bridge_contains(&self, pos: Position) -> bool {
        self.anchor.contains(pos) || self.rect_contains(pos)
    }

    /// Maps a screen position to an item row index inside the panel.
    fn row_at(&self, pos: Position) -> Option<usize> {
        let rect = self.rect?;
        if !rect.contains(pos) {
            return None;
        }
        let first_row = rect.y + 1; // skip top border
        if pos.y < first_row {
            return None;
        }
        let idx = (pos.y - first_row) as usize;
        (idx < self.items.len()).then_some(idx)
    }

    /// Mouse moved: highlight the row under the cursor.
    pub fn on_mouse_move(&mut self, pos: Position) {
        if let Some(idx) = self.row_at(pos) {
            self.selected = Some(idx);
        }
    }

    /// Left click: returns the clicked item index if inside the panel.
    pub fn on_click(&mut self, pos: Position) -> Option<usize> {
        let idx = self.row_at(pos)?;
        self.selected = Some(idx);
        Some(idx)
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let next = match self.selected {
            Some(i) => (i + 1) % self.items.len(),
            None => 0,
        };
        self.selected = Some(next);
    }

    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let prev = match self.selected {
            Some(0) | None => self.items.len() - 1,
            Some(i) => i - 1,
        };
        self.selected = Some(prev);
    }
}

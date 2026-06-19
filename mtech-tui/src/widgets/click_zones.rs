use std::cell::RefCell;

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

/// Makes arbitrary rendered regions clickable without a per-region widget.
/// During draw, register each clickable rect with a string id; in the mouse
/// handler, record the hit; drain it once per frame and dispatch the id.
///
/// Frame order is handle_events → tick → draw, so a click recorded in the
/// mouse handler resolves against the previous frame's rects and is drained at
/// the next tick/draw — no visible lag, no global-bus registration.
#[derive(Default)]
pub struct ClickZones {
    zones: RefCell<Vec<(Rect, String)>>,
    hovered: RefCell<Option<String>>,
    hit: RefCell<Option<String>>,
}

impl ClickZones {
    /// Clear the registered rects at the start of a draw pass.
    pub fn begin(&self) {
        self.zones.borrow_mut().clear();
    }

    /// Register a clickable rect under `id`. Later registrations win on overlap.
    pub fn add(&self, area: Rect, id: impl Into<String>) {
        self.zones.borrow_mut().push((area, id.into()));
    }

    /// Record left-clicks and hover from the tab's `handle_mouse_event`.
    pub fn on_mouse(&self, ev: &MouseEvent) {
        let pos = Position::new(ev.column, ev.row);
        let topmost = self
            .zones
            .borrow()
            .iter()
            .rev()
            .find(|(r, _)| r.contains(pos))
            .map(|(_, id)| id.clone());
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(id) = topmost {
                    *self.hit.borrow_mut() = Some(id);
                }
            }
            MouseEventKind::Moved => *self.hovered.borrow_mut() = topmost,
            _ => {}
        }
    }

    /// Take the pending clicked id, if any. Drain once per frame.
    pub fn take(&self) -> Option<String> {
        self.hit.borrow_mut().take()
    }

    /// The id currently under the cursor, for hover highlighting.
    pub fn hovered(&self) -> Option<String> {
        self.hovered.borrow().clone()
    }
}

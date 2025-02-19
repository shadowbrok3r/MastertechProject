use ratatui::{crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind}, layout::Rect, prelude::Backend, style::Color, Frame};
use ratatui::symbols::border::Set;
use button::State;

pub mod json_viewer;
pub mod button;
pub mod service_form;

pub const SHORTCUT_SET: Set = Set {
    top_left:          "╭",  // Rounded top-left
    top_right:         "╮",  // Rounded top-right
    bottom_left:       "╰",  // Rounded bottom-left
    bottom_right:      "╯",  // Rounded bottom-right
    vertical_left:     "│",
    vertical_right:    "│",
    horizontal_top:    "─",
    horizontal_bottom: "─",
};


pub const SHORTCUT_SET_2: Set = Set {
    top_left:          "◢",
    top_right:         "▜",
    bottom_left:       "▔",
    bottom_right:      "▔",
    vertical_left:     "▏",
    vertical_right:    "▕",
    horizontal_top:    "▔",
    horizontal_bottom: "▔",
};

pub trait HandleWidget<'a>{
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect);
    /// Handle a mouse event
    fn handle_mouse_event(&self, _mouse_event: &MouseEvent) { }
    /// Handle a key event
    fn handle_key_event(&mut self, _key_event: KeyEvent) -> bool { true }
}

pub trait HandleMouse: for<'a> ButtonType<'a> {
    fn handle_mouse_event(&mut self, mouse_event: &MouseEvent) {
        // If we haven’t assigned an area yet, do nothing
        let Some(area) = self.get_area() else { return; };

        let c = mouse_event.column;
        let r = mouse_event.row;
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if the mouse click is within area
                if c >= area.x && c < area.x + area.width &&
                   r >= area.y && r < area.y + area.height {
                    self.set_state(State::Active);
                    self.click(); // calls our on_click callback
                } else {
                    // self.set_state(State::Normal);
                }
            }
            MouseEventKind::Moved => {
                // If you want hover behavior, do it here
                if c >= area.x && c < area.x + area.width &&
                   r >= area.y && r < area.y + area.height {
                    self.set_state(State::Selected);
                } else {
                    self.set_state(State::Normal);
                }
            }
            _ => {}
        }
    }
}

pub trait ButtonType <'a> {
    fn on_click(&self, f: impl FnMut() + 'a) -> Self;
    fn click(&self);
    fn set_state(&self, state: State);
    fn get_area(&self) -> Option<Rect>;
    fn is_active(&self) -> bool;
    fn set_area(&self, area: Rect);
    fn handle_mouse_event(&self, mouse_event: &MouseEvent);
    /// Helper method to get the right colors based on the current state.
    fn colors(&self) -> (Color, Color, Color, Color);
}

/// Shrinks the rect by the specified padding on each side.
/// 
/// The new rect will have:
/// - Its x coordinate increased by `padding_x`.
/// - Its y coordinate increased by `padding_y`.
/// - Its width reduced by `2 * padding_x`.
/// - Its height reduced by `2 * padding_y`.
///
/// This effectively insets the rect equally from all sides.
pub trait ShrinkArea { 
    fn shrink(&self, padding_x: u16, padding_y: u16) -> Rect;
}

impl ShrinkArea for Rect {
    fn shrink(&self, padding_x: u16, padding_y: u16) -> Rect {
        Rect {
            x: self.x.saturating_add(padding_x),
            y: self.y, // keep the top edge fixed
            width: self.width.saturating_sub(2 * padding_x),
            height: self.height.saturating_sub(padding_y), // subtract only from the bottom
        }
    }
}


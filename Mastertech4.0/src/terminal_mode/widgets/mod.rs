use ratatui::{crossterm::event::{KeyEvent, MouseEvent}, layout::Rect, prelude::Backend, style::Color, Frame};
use ratatui::symbols::border::Set;
use button::State;

pub mod json_viewer;
pub mod button;
pub mod service_form;
pub mod input_field;

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


pub const _SHORTCUT_SET_2: Set = Set {
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


pub trait ButtonType <'a> {
    // fn on_click(&self, f: impl FnMut(&mut T) + 'a);
    fn click(&self);
    fn set_state(&self, state: State);
    // #[allow(unused)]
    fn _get_area(&self) -> Option<Rect>;
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


use ratatui::{crossterm::event::{KeyEvent, MouseEvent}, layout::Rect, prelude::Backend, style::Color, Frame};
use ratatui::symbols::border::Set;
use button::ButtonState;

// pub mod autocomplete;
pub mod json_viewer;
pub mod button;
pub mod input_field;
pub mod autocomplete_input;
// pub mod calendar;

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

pub trait HandleWidget<'a> { // : HandleClientWidget
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect);
    /// Handle a mouse event
    fn handle_mouse_event(&self, _mouse_event: &MouseEvent) { }
    /// Handle a key event
    fn handle_key_event(&mut self, _key_event: KeyEvent) -> bool { true }

    // fn do_the_websocket_thing(&mut self) {
    //     self.do_thing(|w| {
    //         String::new()
    //     });
    // }
}

// pub enum WhichAmI {
//     Master,
//     Client
// }

// pub trait HandleClientWidget {
//     fn do_thing<T: FnOnce(WhichAmI) -> String >(&mut self, add_contents: T) {
//         let who_am_i = WhichAmI::Client;

//         let x = add_contents(who_am_i);

//     }
// }


pub trait ButtonType <'a> {
    // fn on_click(&self, f: impl FnMut(&mut T) + 'a);
    /// For when something (mouse, keyboard) triggers a "click" on this Button.
    fn click(&self) {
        self.set_state(ButtonState::Active);
    }

    fn set_state(&self, state: ButtonState);
    // #[allow(unused)]
    fn get_area(&self) -> Option<Rect>;
    fn is_active(&self) -> bool;
    fn set_area(&self, area: Rect);
    fn handle_mouse_event(&self, mouse_event: &MouseEvent);
    fn handle_key_event(&self, _key_event: &KeyEvent) -> bool {false}
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


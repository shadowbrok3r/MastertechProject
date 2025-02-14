use ratatui::{crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind}, layout::Rect, prelude::Backend, Frame};
use ratatui::symbols::border::Set;
use button::State;

pub mod json_viewer;
pub mod button;

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
    fn handle_mouse_event(&mut self, _mouse_event: MouseEvent) { }
    /// Handle a key event
    fn handle_key_event(&mut self, _key_event: KeyEvent) { }
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
                    self.set_state(State::Normal);
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
    fn on_click(self, f: impl FnMut() + 'a) -> Self;
    fn click(&mut self);
    fn set_state(&mut self, state: State);
    fn set_area(&mut self, area: Rect) -> &mut Self;
    fn get_area(&self) -> Option<Rect>;
}


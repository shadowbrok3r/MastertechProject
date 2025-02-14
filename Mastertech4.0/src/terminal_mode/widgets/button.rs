use ratatui::{
    buffer::Buffer, crossterm::event::{MouseButton, MouseEvent, MouseEventKind}, layout::Rect, style::{Color, Style}, text::Line, widgets::WidgetRef
};

use crate::terminal_mode::styling::TURQUOISE;
use std::fmt::Debug;

use super::ButtonType;


/// ------------------------------
/// Custom Button widget
/// ------------------------------
/// Holds info for each button:
/// - `label`: what text to display
/// - `state`: normal, selected, active, etc.
/// - `theme`: coloring for the button
/// - `area`: updated at runtime (where the button was drawn)
/// - `on_click`: optional callback to do something when the button is clicked
// #[derive(Debug)]
pub struct Button<'a> {
    label: Line<'a>,
    theme: Theme,
    state: State,
    pub area: Option<Rect>,
    on_click: Option<Box<dyn FnMut() + 'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Normal,
    Selected,
    Active,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub text: Color,
    pub background: Color,
    pub highlight: Color,
    pub shadow: Color,
}

impl<'a> Button<'a> {
    pub fn new<T: Into<Line<'a>>>(label: T) -> Self {
        Button {
            label: label.into(),
            theme: TURQUOISE,
            state: State::Normal,
            area: None,
            on_click: None,
        }
    }

    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Helper method to get the right colors based on the current state.
    const fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme;
        match self.state {
            State::Normal => (t.background, t.text, t.shadow, t.highlight),
            State::Selected => (t.highlight, t.text, t.shadow, t.highlight),
            State::Active => (t.background, t.text, t.highlight, t.shadow),
        }
    }
    
    pub fn handle_mouse_event(&mut self, mouse_event: &MouseEvent) {
        // If we haven’t assigned an area yet, do nothing
        let Some(area) = self.area else { return; };

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

impl <'a> ButtonType<'a> for Button<'a> {
    fn on_click(mut self, f: impl FnMut() + 'a) -> Self {
        log::info!("on_click event fired");
        self.on_click = Some(Box::new(f));
        self
    }
    
    /// For when something (mouse, keyboard) triggers a "click" on this Button.
    fn click(&mut self) {
        if let Some(callback) = self.on_click.as_mut() {
            log::info!("click callback fired");
            (callback)(); // run the user’s callback
        }
    }

    fn set_state(&mut self, state: State) {
        self.state = state;
    }

    fn set_area(&mut self, area: Rect) -> &mut Self {
        self.area = Some(area);
        self
    }

    fn get_area(&self) -> Option<Rect> {
        self.area
    }
}


// impl<'a> Widget for Button<'a> {
//     fn render(self, area: Rect, buf: &mut Buffer) {
//         let (background, text, shadow, highlight) = self.colors();
//         // Fill area with background + text color.
//         buf.set_style(area, Style::default().bg(background).fg(text));
//         // If there's room, draw top highlight line.
//         if area.height > 2 {
//             let top_str = "▔".repeat(area.width as usize);
//             buf.set_string(
//                 area.x,
//                 area.y,
//                 top_str,
//                 Style::default().fg(highlight).bg(background),
//             );
//         }
//         // If there's room, draw bottom shadow line.
//         if area.height > 1 {
//             let bot_str = "▁".repeat(area.width as usize);
//             buf.set_string(
//                 area.x,
//                 area.y + area.height - 1,
//                 bot_str,
//                 Style::default().fg(shadow).bg(background),
//             );
//         }
//         // Center the label.
//         let label_x = area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2;
//         let label_y = area.y + (area.height.saturating_sub(1)) / 2;
//         buf.set_line(label_x, label_y, &self.label, area.width);
//     }
// }

impl <'a> WidgetRef for Button<'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        // Fill area with background + text color.
        buf.set_style(area, Style::default().bg(background).fg(text));

        // If there's room, draw top highlight line.
        if area.height > 2 {
            let mut top_str = String::new();
            top_str.insert(0, '┌'); 
            /* ┐ ┬ └ ┘ └ ┘ */
            // top_str.insert(0, '┌');
            let top_str = "▔".repeat(area.width as usize);
            
            buf.set_string(
                area.x,
                area.y,
                top_str,
                Style::default().fg(highlight).bg(background),
            );
        }
        // If there's room, draw bottom shadow line.
        if area.height > 1 {
            let mut bot_str = "▁".repeat(area.width as usize - 1);
            bot_str.push_str("◢");
            
            buf.set_string(
                area.x,
                area.y + area.height - 1,
                bot_str,
                Style::default().fg(shadow).bg(background),
            );
        }

        // Center the label.
        let label_x = area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2;
        let label_y = area.y + (area.height.saturating_sub(1)) / 2;
        buf.set_line(label_x, label_y, &self.label, area.width);
    }
}

/*
use super::{colors::{Theme, BLUE}, State};
use ratatui::{
    style::{Color, Style},
    buffer::Buffer, layout::Rect, text::Line, widgets::Widget
};

#[derive(Debug, Clone)]
pub struct Button<'a> {
    pub label: Line<'a>,
    pub theme: Theme,
    pub state: State,
}


/// A button with a label that can be themed.
impl<'a> Button<'a> {
    pub fn new<T: Into<Line<'a>>>(label: T) -> Self {
        Button {
            label: label.into(),
            theme: BLUE,
            state: State::Normal,
        }
    }

    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }
}

impl<'a> Widget for Button<'a> {
    #[allow(clippy::cast_possible_truncation)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        buf.set_style(area, Style::new().bg(background).fg(text));

        // render top line if there's enough space
        if area.height > 2 {
            buf.set_string(
                area.x,
                area.y,
                "▔".repeat(area.width as usize),
                Style::new().fg(highlight).bg(background),
            );
        }
        // render bottom line if there's enough space
        if area.height > 1 {
            buf.set_string(
                area.x,
                area.y + area.height - 1,
                "▁".repeat(area.width as usize),
                Style::new().fg(shadow).bg(background),
            );
        }
        // render label centered
        buf.set_line(
            area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2,
            area.y + (area.height.saturating_sub(1)) / 2,
            &self.label,
            area.width,
        );
    }
}

impl Button<'_> {
    const fn colors(&self) -> (Color, Color, Color, Color) {
        let theme = self.theme;
        match self.state {
            State::Normal => (theme.background, theme.text, theme.shadow, theme.highlight),
            State::Selected => (theme.highlight, theme.text, theme.shadow, theme.highlight),
            State::Active => (theme.background, theme.text, theme.highlight, theme.shadow),
        }
    }
}

*/
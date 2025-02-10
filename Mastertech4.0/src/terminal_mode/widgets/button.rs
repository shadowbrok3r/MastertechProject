use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};
use std::fmt::Debug;

use crate::terminal_mode::{colors::TURQUOISE, App};


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
    area: Option<Rect>,
    on_click: Option<Box<dyn FnMut(&mut App)>>,
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
    pub fn new<T: Into<Line<'a>>>(label: T, area: Rect) -> Self {
        Button {
            label: label.into(),
            theme: TURQUOISE,
            state: State::Normal,
            area: Some(area),
            on_click: None,
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

    /// Helper method to get the right colors based on the current state.
    const fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme;
        match self.state {
            State::Normal => (t.background, t.text, t.shadow, t.highlight),
            State::Selected => (t.highlight, t.text, t.shadow, t.highlight),
            State::Active => (t.background, t.text, t.highlight, t.shadow),
        }
    }

    pub fn area(&self) -> Option<Rect> {
        self.area
    }

    // pub fn click(&mut self)
}

impl<'a> Widget for Button<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        // Fill area with background + text color.
        buf.set_style(area, Style::default().bg(background).fg(text));

        // If there's room, draw top highlight line.
        if area.height > 2 {
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
            let bot_str = "▁".repeat(area.width as usize);
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
#[cfg(feature="term")]
use {
    super::{colors::{Theme, BLUE}, State},
    ratatui::{
        style::{Color, Style},
        buffer::Buffer, layout::Rect, text::{Line, Span}, widgets::Widget
    }
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

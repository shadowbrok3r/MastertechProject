use mtech_tui::styling::{APP_BACKGROUND, THEME};
use mtech_tui::widgets::HandleWidget;
use ratatui::{
    layout::Rect,
    prelude::Backend,
    style::Style,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

/// Stand-in tab for views whose terminal port has not landed yet. Keeps the
/// `Tab` dispatch match exhaustive while individual tabs are built out.
#[allow(dead_code)]
pub struct PlaceholderTab {
    title: String,
}

impl PlaceholderTab {
    #[allow(dead_code)]
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into() }
    }
}

impl<'a> HandleWidget<'a> for PlaceholderTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(false))
            .title(self.title.clone());
        let body = Paragraph::new(format!(
            "{} — terminal view not yet implemented.",
            self.title
        ))
        .block(block)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(THEME.text_muted).bg(APP_BACKGROUND));
        f.render_widget(body, area);
    }
}

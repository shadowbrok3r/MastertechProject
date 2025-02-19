use ratatui::{layout::Rect, prelude::Backend, style::{Color, Style}, widgets::{Block, Borders}, Frame};

use crate::terminal_mode::widgets::HandleWidget;

////////////////////////////////
// Sysinfo TAB with Buttons
////////////////////////////////
/// Let's say we have a subcomponent called ScriptsTab
pub struct SysinfoTab;

impl<'a> SysinfoTab {
    pub fn new() -> Self {
        Self { }
    }
}

impl <'a> HandleWidget <'_> for SysinfoTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
        .borders(Borders::ALL)
        .title("System Info")
        .border_style(Style::default().fg(Color::LightCyan));

        f.render_widget(block, area);
    }
}
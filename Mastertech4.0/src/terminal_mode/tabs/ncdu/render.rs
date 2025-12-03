
use ratatui::crossterm::event::Event;
use ratatui::{prelude::*, widgets::{Block, BorderType, Borders, Clear, Paragraph, FrameExt}};
use crate::terminal_mode::widgets::HandleWidget;
use super::{get_file_content, NcduTab};

impl<'a> HandleWidget <'a> for NcduTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let chunks = self.layout.split(area);

        // Left: directory tree - use render_widget_ref for WidgetRef types
        f.render_widget_ref(self.explorer.widget(), chunks[0]);

        // Right: file preview / info panel
        f.render_widget(Clear, chunks[1]);
        let preview = get_file_content(self.explorer.current()).unwrap_or_else(|_| std::borrow::Cow::Borrowed("<unable to read file>"));
        let para = Paragraph::new(preview).block(
            Block::default()
                .title("Preview")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        );
        f.render_widget(para, chunks[1]);
    }

    fn handle_mouse_event(&self, _mouse_event: &ratatui::crossterm::event::MouseEvent) {
        
    }
    
    fn handle_key_event(&mut self, key_event: ratatui::crossterm::event::KeyEvent) -> bool {
        let _ = self.explorer.handle(&Event::Key(key_event));

        return true;
    }
}
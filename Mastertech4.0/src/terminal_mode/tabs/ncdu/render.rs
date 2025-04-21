
use ratatui::{crossterm::event::KeyCode, prelude::*, widgets::{Block, BorderType, Borders, Clear, Paragraph}};
use crate::terminal_mode::widgets::HandleWidget;
use super::{get_file_content, NcduTab};

impl<'a> HandleWidget <'a> for NcduTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let chunks = self.layout.split(area);

        // Left: directory tree
        f.render_widget(&self.explorer.widget(), chunks[0]);

        // Right: file preview / info panel
        f.render_widget(Clear, chunks[1]);
        let preview = get_file_content(self.explorer.current()).unwrap_or_else(|_| std::borrow::Cow::Borrowed("<unable to read file>"));
        let para = Paragraph::new(preview).block(
            Block::default()
                .title("Preview")
                .borders(Borders::ALL)
                .border_type(BorderType::Double),
        );
        f.render_widget(para, chunks[1]);
    }

    fn handle_mouse_event(&self, _mouse_event: &crossterm::event::MouseEvent) {
        
    }
    
    fn handle_key_event(&mut self, key_event: ratatui::crossterm::event::KeyEvent) -> bool {
        // let event = Event::Key(key_event);

        let input = match key_event.code {
            KeyCode::Up => ratatui_explorer::Input::Up,
            KeyCode::Down => ratatui_explorer::Input::Down,
            KeyCode::Left => ratatui_explorer::Input::Left,
            KeyCode::Right => ratatui_explorer::Input::Right,
            KeyCode::PageUp => ratatui_explorer::Input::PageUp,
            KeyCode::PageDown => ratatui_explorer::Input::PageDown,
            KeyCode::Home => ratatui_explorer::Input::Home,
            KeyCode::End => ratatui_explorer::Input::End,
            _ => ratatui_explorer::Input::None
        };

        let _ = self.explorer.handle(input);

        return true;
    }
}
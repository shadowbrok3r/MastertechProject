use ratatui::{layout::Rect, prelude::Backend, widgets::{Block, Borders, List, ListItem, Paragraph}, Frame};

// Trait for renderable widgets
pub trait WidgetRenderer {
    fn render_widget<B: Backend>(&self, frame: &mut Frame, area: Rect);
}

// Implement WidgetRenderer for String
impl WidgetRenderer for String {
    fn render_widget<B: Backend>(&self, frame: &mut Frame, area: Rect) {
        let paragraph = Paragraph::new(self.as_str())
            .block(
                Block::default()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .borders(Borders::ALL)
                .title("Paragraph")
            );
        frame.render_widget(paragraph, area);
    }
}

// Implement WidgetRenderer for Vec<String>
impl WidgetRenderer for Vec<String> {
    fn render_widget<B: Backend>(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.iter().map(|item| ListItem::new(item.as_str())).collect();
        let list = List::new(items)
            .block(
                Block::default()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .borders(Borders::ALL)
                .title("List")
            );
        frame.render_widget(list, area);
    }
}
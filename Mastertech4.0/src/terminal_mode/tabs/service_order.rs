use ratatui::{layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::Style, text::Line, widgets::{Block, Borders, List, ListItem, Paragraph}, Frame};
use ratatui::prelude::*;
use tui_scrollview::{ScrollView, ScrollbarVisibility};
use crate::terminal_mode::{colors::{C_SPRINGGREEN, TURQUOISE}, widgets::button::Button, App, C_DEEPPINK, C_MEDIUMSLATEBLUE};

////////////////////////////////
// TUR SHEET TAB with Input Field
////////////////////////////////
pub fn render_tur_sheet_tab<B: Backend>(app: &mut App, f: &mut Frame, area: Rect) {
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    // (A) Input row + 2 buttons
    let input_button_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(70),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ])
        .split(vertical_chunks[0]);

    // Input field
    let width = input_button_chunks[0].width.saturating_sub(2);
    let scroll_offset = app.input.visual_scroll(width as usize);

    let input_widget = Paragraph::new(app.input.value())
        .style(Style::default().fg(C_DEEPPINK))
        .scroll((0, scroll_offset as u16))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Input")
                .border_style(Style::default().fg(C_MEDIUMSLATEBLUE))
        );
    f.render_widget(input_widget, input_button_chunks[0]);

    // Cursor in input
    let cursor_x = input_button_chunks[0].x + ((app.input.visual_cursor()).max(scroll_offset) - scroll_offset) as u16 + 1;
    let cursor_y = input_button_chunks[0].y + 1;
    f.set_cursor_position(Position::new(cursor_x, cursor_y));

    // 'Get Ticket' button
    let get_ticket_button = Button::new(Line::from("Get Ticket"), input_button_chunks[1])
        .theme(TURQUOISE)
        .state(app.get_ticket_button_state);

    f.render_widget(get_ticket_button, input_button_chunks[1]);
    app.get_ticket_button_area = Some(input_button_chunks[1]);

    // 'Submit Ticket' button
    let submit_button = Button::new(Line::from("Submit"), input_button_chunks[2])
        .theme(TURQUOISE)
        .state(app.submit_ticket_button_state);

    // app.buttons.push(submit_button);
    
    f.render_widget(submit_button, input_button_chunks[2]);
    app.submit_ticket_button_area = Some(input_button_chunks[2]);

    // (B) Logs + JSON
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(vertical_chunks[1]);

    // Logs
    let items: Vec<ListItem> = app.logs.iter().map(|log| {
        ListItem::new(log.clone()).style(
            Style::default().fg(Color::Rgb(224, 255, 255))
        )
    }).collect();

    let logs_list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Logs")
            .border_style(Style::default().fg(C_SPRINGGREEN))
    );
    f.render_widget(logs_list, horizontal_chunks[0]);

    // JSON viewer
    let text = app.json_widget.render_text();
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Json Viewer")
                .border_style(Style::default().fg(C_DEEPPINK))
        );

    let size = Size {
        width: horizontal_chunks[1].width,
        height: horizontal_chunks[1].height,
    };
    let mut scroll_view = ScrollView::new(size)
        .scrollbars_visibility(ScrollbarVisibility::Always);
    scroll_view.render_widget(paragraph, scroll_view.area());
    f.render_stateful_widget(scroll_view, horizontal_chunks[1], &mut app.json_scroll_state);
}
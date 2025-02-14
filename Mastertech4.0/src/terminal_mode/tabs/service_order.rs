use crossbeam::channel::{self, Receiver, Sender};
use crate::terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::CATPPUCCINTHEME};
use database::schema::{prestashop_schema::{self}, utilities::get_prestashop_payload};
use ratatui::{crossterm::event::{Event, KeyCode, KeyEvent}, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::Style, widgets::{Block, Borders, List, ListItem, ListState, Paragraph}, Frame};
use serde_json::Value;
use crate::terminal_mode::{widgets::{button::Button, json_viewer::JsonWidget, ButtonType, HandleWidget}, C_DEEPPINK};
use ratatui::crossterm::event::MouseEvent;
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};
use tui_input::{backend::crossterm::EventHandler, Input};
use ratatui::prelude::*;

////////////////////////////////
// TUR SHEET TAB with SERVICE NUM INPUT
////////////////////////////////
/// ServiceTab Component
pub struct ServiceTab<'a> {
    // Prestashop
    prestashop_api_tx: Sender<prestashop_schema::PrestashopPayload>,
    prestashop_api_rx: Receiver<prestashop_schema::PrestashopPayload>,

    get_ticket_button: Button<'a>,
    submit_button: Button<'a>,
    input: Input,
    logs: Vec<String>,
    log_state: ListState,

    // JSON
    pub json_widget: JsonWidget,
    json_scroll_state: ScrollViewState,
    pub effect_stage: EffectStage<UniqueEffectId>
}

impl<'a> ServiceTab<'a> {
    pub fn new() -> Self {
        let (prestashop_api_tx, prestashop_api_rx) = channel::unbounded();

        Self {
            get_ticket_button: Button::new("Get Ticket")
                .on_click(|| {
                    log::info!("Getting ticket");
                }).theme(CATPPUCCINTHEME),
            submit_button: Button::new("Submit")
                .on_click(|| {
                    log::info!("Submitting TUR");
                }).theme(CATPPUCCINTHEME),
            input: Input::default(),
            logs: Vec::new(),
            json_widget: JsonWidget::default(),
            json_scroll_state: ScrollViewState::default(),
            prestashop_api_tx,
            prestashop_api_rx,
            effect_stage: EffectStage::default(),
            log_state: ListState::default(),
        }
    }

    fn log_message(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }

    fn log_json(&mut self, value: Value) {
        self.json_widget = JsonWidget::new(value);
    }

    pub fn receive_ticket(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Ok(data) = self.prestashop_api_rx.try_recv() {
            self.log_message(&serde_json::to_string(&data)?);
            self.log_json(serde_json::to_value(&data)?);
        }
        Ok(())
    }

    fn get_ticket(&self, service_number: &str) {
        let tx = self.prestashop_api_tx.clone();
        let input = service_number.to_string();
        log::info!("Getting payload with {input}");
        if !input.is_empty() {
            tokio::spawn(async move {
                let prestashop_order = get_prestashop_payload(&input).await?;
                tx.try_send(prestashop_order)?;
                Ok::<(), anyhow::Error>(())
            });
        }
    }
}

impl <'a> HandleWidget <'_> for ServiceTab <'_> {
    /// Draw the entire ServiceTab, including its buttons
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        
        // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        let fx_duration = tachyonfx::Duration::from_millis(16);
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        self.effect_stage.process_effects(fx_duration, f.buffer_mut(), area);


        // Divide the area into vertical chunks (input row + main content)
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(area);
    
        // (A) Input row and 2 buttons
        let input_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(14),
                Constraint::Percentage(2),
                Constraint::Percentage(14),
            ])
            .split(vertical_chunks[0]);
    
        // -----------------------------
        // INPUT FIELD
        // -----------------------------
        // let width = input_chunks[0].width.saturating_sub(2);
        // let scroll_offset = self.input.visual_scroll(width as usize);
        let width = input_chunks[0].width.max(3) - 3; // keep 2 for borders and 1 for cursor
        let scroll = self.input.visual_scroll(width as usize);
        let input_widget = Paragraph::new(self.input.value())
            .style(Style::default().fg(C_DEEPPINK))
            .scroll((0, scroll as u16))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Input")
                    // .border_style(Style::default().fg(C_MEDIUMSLATEBLUE))
                    .border_type(ratatui::widgets::BorderType::Rounded)
            );
        let input_rect = input_chunks[0];
        f.render_widget(input_widget, input_rect);
        // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
        f.set_cursor_position((
            // Put cursor past the end of the input text
            input_chunks[0].x
                + ((self.input.visual_cursor()).max(scroll) - scroll) as u16
                + 1,
            // Move one line down, from the border to the input line
            input_chunks[0].y + 1,
        ));
    
        // -----------------------------
        // (B) Logs and JSON viewer
        // -----------------------------
        let horizontal_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(80),
                Constraint::Percentage(20),
            ])
            .split(vertical_chunks[1]);
    
        let json_area_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(10),
                Constraint::Percentage(90),
            ])
            .split(horizontal_chunks[0]);

        let json_view_title_area = json_area_chunks[0];
        let json_view_area = json_area_chunks[1];
        let log_area = horizontal_chunks[1];

        // Logs
        let items: Vec<ListItem> = self.logs.iter().map(|log| {
            ListItem::new(log.clone())
                .style(Style::default().fg(Color::Rgb(224, 255, 255)))
        }).collect();
    
        let logs_list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Logs")
                // .border_style(Style::default().fg(C_SPRINGGREEN))
                .border_type(ratatui::widgets::BorderType::Rounded)
        );

        let json_viewer_title = Block::new()
            .title(Line::from("Json View").centered())
            .borders(Borders::BOTTOM)
            .border_type(ratatui::widgets::BorderType::Rounded);

        // JSON viewer
        let text = self.json_widget.render_text();
        let mut borders = Borders::RIGHT;
        borders.set(Borders::LEFT, true);
        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(borders)
                .bg(Color::Rgb(12,12,16))
                // .border_style(Style::default().fg(C_DEEPPINK))
                .border_type(ratatui::widgets::BorderType::Rounded)
        );

        let mut scroll_view = ScrollView::new(
            Size { width: json_view_area.width, height: json_view_area.height }
        ).scrollbars_visibility(ScrollbarVisibility::Automatic);

        // -----------------------------
        // (C) Buttons: Get Ticket & Submit
        // -----------------------------
        let get_ticket_btn_rect = input_chunks[2];
        let submit_btn_rect = input_chunks[4];

        self.get_ticket_button.set_area(get_ticket_btn_rect);
        self.submit_button.set_area(submit_btn_rect);

        f.render_widget(&self.get_ticket_button, input_chunks[2]);
        f.render_widget(&self.submit_button, input_chunks[4]);

        // Render JSON viewer scroll view.
        json_viewer_title.render(json_view_title_area, f.buffer_mut());
        paragraph.render(scroll_view.area(), scroll_view.buf_mut());
        f.render_stateful_widget(scroll_view, json_view_area, &mut self.json_scroll_state);
        f.render_stateful_widget(logs_list, log_area, &mut self.log_state);
    }
    
    /// Handle a mouse event, see if it hits our get_ticket_button or submit_button
    fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollDown => self.json_scroll_state.scroll_down(),
            ratatui::crossterm::event::MouseEventKind::ScrollUp => self.json_scroll_state.scroll_up(),
            ratatui::crossterm::event::MouseEventKind::ScrollLeft => self.json_scroll_state.scroll_left(),
            ratatui::crossterm::event::MouseEventKind::ScrollRight => self.json_scroll_state.scroll_right(),
            _ => {
                self.get_ticket_button.handle_mouse_event(&mouse_event);
                self.submit_button.handle_mouse_event(&mouse_event);
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // Send keystroke to input field
        let _ = self.input.handle_event(&Event::Key(key_event));
        match key_event.code {
            KeyCode::Enter => {
                let user_input = self.input.value();
                self.get_ticket(user_input);
                self.log_message(&format!("(Enter) 'Get Ticket' with input: {}", user_input));
            }
            KeyCode::Down => {
                // Move highlight in JSON widget, etc.
                self.json_widget.next_edit();
            }
            KeyCode::Up => {
                self.json_widget.prev_edit();
            }
            _ => ()
        }
    }
}
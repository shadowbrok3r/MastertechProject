use displays::remote_viewer::ratagui::TerminalEvent;
use ratatui::{buffer::Buffer, layout::{Constraint, Direction, Layout, Margin, Position, Rect}, prelude::Backend, style::Stylize, widgets::{Block, Clear, Paragraph, WidgetRef}, Frame};
use crate::terminal_mode::{data::LocalTermEvent, styling::CATPPUCCIN, widgets::{ButtonType, ShrinkArea}};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use super::{PageState, WebconsoleTab};


impl <'a> WebconsoleTab <'a> {
    fn draw_page(&mut self, f: &mut Frame, area: Rect) {
        // Render right half based on page state
        match &self.page_state {
            PageState::None => {
                let placeholder = Paragraph::new("Select a client to view remote terminal")
                    .block(Block::default().bg(CATPPUCCIN.base))
                    .centered();
                placeholder.render_ref(area, f.buffer_mut());
            },
            PageState::RemoteTerminal(connection_string) => {
                // Poll buffer_rx for new frames
                // if let Some(ref mut buffer_rx) = self.buffer_rx {
                //     while let Ok((frame_count, buffer)) = buffer_rx.try_recv() {
                //         log::info!("Rendering remote buffer, frame_count={}", frame_count);
                //         self.remote_buffer = Some(buffer);
                //     }
                // }

                if let Some(buffer) = &self.remote_buffer {
                    self.client_area = area;
                    // Draw a background block
                    f.render_widget(
                        Block::default()
                        .bg(CATPPUCCIN.base)
                        .border_type(ratatui::widgets::BorderType::QuadrantOutside),
                        area,
                    );
                    // f.render_widget(Clear, area);
                    // f.buffer_mut().merge(buffer);
                    // Resize and copy buffer contents into the frame
                    let inner_area = area.inner(Margin { horizontal: 1, vertical: 1 });
                    for (i, cell) in buffer.content().iter().enumerate() {
                        let x = (i % buffer.area.width as usize) as u16;
                        let y = (i / buffer.area.width as usize) as u16;
                        if x < inner_area.width && y < inner_area.height {
                            if let Some(target_cell) = f.buffer_mut().cell_mut(Position {
                                x: inner_area.x + x,
                                y: inner_area.y + y,
                            }) {
                                *target_cell = cell.clone();
                            }
                        }
                    }
                }
            },
        }
    }

    // Helper to get inner_area (could be cached or passed differently)
    fn get_inner_area(&self) -> Rect {
        // let full_area = Rect::new(0, 0, 100, 50); // Replace with actual area from draw
        self.client_area.inner(Margin { horizontal: 1, vertical: 1 })
    }
}
/// Implement the HandleWidget trait for ServiceFormTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> crate::terminal_mode::widgets::HandleWidget<'a> for WebconsoleTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.receive();
        // Split area into buttons (left) and logs (right)
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15), // Left: Buttons
                Constraint::Percentage(85), // Right: Logs
            ])
            .split(area);

        let left_half = main_chunks[0];
        let right_half = main_chunks[1];

        let left_side_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(2),
            Constraint::Percentage(98),
        ])
        .split(left_half);

        let para = Paragraph::new("Clients")
            .block(Block::default().bg(CATPPUCCIN.base))
            .centered();

        para.render_ref(left_side_chunks[0], f.buffer_mut());

        // Create grid layout for buttons
        let button_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                vec![
                    Constraint::Length(4);
                    if self.ws_clients.is_empty() { 1 } else { self.ws_clients.len() }     
                ])
            .split(left_side_chunks[1]);
        
        if self.ws_clients.is_empty() {
            f.render_widget(&self.get_clients_btn, button_grid[0].shrink(4, 1));
        } else {
            for (i, (_, btn)) in self.ws_clients.iter().enumerate() {
                f.render_widget(btn, button_grid[i].shrink(4, 1));
            }
        }

        self.draw_page(f, right_half);
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let c = mouse_event.column;
        let r = mouse_event.row;
        let mouse_position = Position::new(c, r);

        
        match mouse_event.kind {
            crossterm::event::MouseEventKind::Down(_) => {
                if let (Some(_), PageState::RemoteTerminal(_)) = (&self.remote_buffer, &self.page_state) {
                    // let event = TerminalEvent::MouseClick { x: c, y: r };
                    // let _ = self.event_tx.try_send(event);
                    let inner_area = self.get_inner_area(); // Calculate dynamically if needed
                    if inner_area.contains(mouse_position) {
                        let adjusted_x = c - inner_area.x;
                        let adjusted_y = r - inner_area.y;
                        let event = TerminalEvent::MouseClick { x: adjusted_x, y: adjusted_y };
                        log::info!("Sent mouse event: {:?}", event);
                        let _ = self.event_tx.try_send(event);
                    }
                }
            },
            _ => {}
        }

        self.get_clients_btn.handle_mouse_event(mouse_event);
        for (_, btn) in self.ws_clients.iter() {
            btn.handle_mouse_event(mouse_event);
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        if let Some(_) = &self.remote_buffer {
            let local_term_event = LocalTermEvent::try_from(key_event);
            if let Ok(evt) = local_term_event {
                let _ = self.event_tx.try_send(evt.0);
            }
        }

        false
    }
}
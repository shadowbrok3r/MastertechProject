use ratatui::{prelude::{StatefulWidget, Backend}, crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent}, layout::{Constraint, Direction, Layout, Rect, Size}, text::Line, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::terminal_mode::{styling::CATPPUCCIN, widgets::{button::ButtonState, ButtonType, HandleWidget, ShrinkArea, SHORTCUT_SET}};
use tui_scrollview::ScrollView;
use super::ServiceFormTab;

// Define a virtual height for the service form content.
pub const SERVICE_FORM_VIRTUAL_HEIGHT: u16 = 46;

/// Implement the HandleWidget trait for ServiceFormTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> HandleWidget<'a> for ServiceFormTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {   
        self.service_form_area.replace(Some(area));
        // Divide the area into vertical chunks (input row + main content)
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(area);
    
        let area_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(5),
                Constraint::Percentage(95),
            ])
            .split(vertical_chunks[1]);

        let title_area = area_chunks[0];
        let service_form_area = area_chunks[1];

        let title = Block::new()
            .title(Line::from("Service Form").centered())
            .borders(Borders::BOTTOM)
            .border_type(ratatui::widgets::BorderType::Rounded);

        // Create a scroll view with a fixed virtual content size.
        // This ensures that even if `service_form_area` (the visible area) is small,
        // the service form widget is rendered into a larger virtual buffer.
        let virtual_size = Size {
            width: service_form_area.width,
            height: SERVICE_FORM_VIRTUAL_HEIGHT,
        };

        let mut scroll_view = ScrollView::new(virtual_size);

        // Render JSON viewer scroll view.
        title.render_ref(title_area, f.buffer_mut());

        // For brevity, your real code might define constraints differently
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // row 1: Service Number field / Get Ticket / submit button
                Constraint::Length(3),  // row 2: Customer info
                Constraint::Length(3),  // row 3: Sales/Tech
                Constraint::Length(3),  // row 4: first row of buttons
                Constraint::Length(1),  // row 5: spacer
                Constraint::Length(3),  // row 6: second row of buttons
                Constraint::Max(10),    // row 7: multiline text fields
            ])
            .split(scroll_view.area());

        // Row 1: Customer Name | Customer Phone
        let row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[0]);

        self.order_number.render_ref(row1[0], scroll_view.buf_mut());

        let get_ticket_btn_area = row1[1].shrink(4, 0);

        self.get_ticket_btn.render_ref(get_ticket_btn_area, scroll_view.buf_mut());

        // -- Submit button
        let submit_btn_area = row1[3].shrink(4, 0);
        self.submit_btn.render_ref(submit_btn_area, scroll_view.buf_mut());

        // Row 2: Salesman Name | Technician Name
        let row2 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[1]);

        // -- Customer Name
        self.customer_name.render_ref(row2[0], scroll_view.buf_mut());

        // -- Customer Phone
        self.customer_phone.render_ref(row2[1], scroll_view.buf_mut());

        // Row 3: Get Keys Button | Check SEB Button
        let row3 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[2]);

        // -- Salesman
        self.salesman_name.render_ref(row3[0], scroll_view.buf_mut());

        // -- Technician
        self.technician_name.render_ref(row3[1], scroll_view.buf_mut());

        let row4 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[3]);

        // Left button
        let get_keys_btn_area = row4[0].shrink(4, 0);
        self.get_keys_btn.render_ref(get_keys_btn_area, scroll_view.buf_mut());

        // Right button
        let check_seb_btn_area = row4[1].shrink(4, 0);
        self.check_seb_btn.render_ref(check_seb_btn_area, scroll_view.buf_mut());

        // Row 6: CheckIn Notes | Recommendations
        let row5 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[5]);

        let webroot_key_btn_area = row5[0].shrink(4, 0);
        self.webroot_key_btn.render_ref( webroot_key_btn_area, scroll_view.buf_mut());

        let superanti_key_btn_area = row5[1].shrink(4, 0);
        self.superanti_key_btn.render_ref( superanti_key_btn_area, scroll_view.buf_mut());

        // Row 7: CheckIn Notes | Recommendations
        let row6 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[6]);

        // -- CheckIn Notes
        self.checkin_notes.render_ref(row6[0], scroll_view.buf_mut());

        // -- Recommendations
        self.recommendations.render_ref(row6[1], scroll_view.buf_mut());

        scroll_view.render(service_form_area, f.buffer_mut(), &mut self.scroll_state.borrow_mut());
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollDown => self.scroll_state.borrow_mut().scroll_down(),
            ratatui::crossterm::event::MouseEventKind::ScrollUp => self.scroll_state.borrow_mut().scroll_up(),
            ratatui::crossterm::event::MouseEventKind::ScrollLeft => self.scroll_state.borrow_mut().scroll_left(),
            ratatui::crossterm::event::MouseEventKind::ScrollRight => self.scroll_state.borrow_mut().scroll_right(),
            _ => {
                let scroll_state = self.scroll_state.borrow();
                let scroll_x = scroll_state.offset().x;
                let scroll_y = scroll_state.offset().y;
            
                let service_form_area = self.service_form_area.borrow().unwrap_or(Rect::new(0, 0, 0, 0));
            
                // Calculate total offset (service_form_area.y + area_chunks[0].height)
                let vertical_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(2), Constraint::Min(1)])
                    .split(service_form_area); // Approximate original area
                let area_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(5), Constraint::Percentage(95)])
                    .split(vertical_chunks[1]);
                let total_offset = service_form_area.y + area_chunks[0].height + 2;
            
                let inside = mouse_event.column >= service_form_area.x
                    && mouse_event.column < service_form_area.x + service_form_area.width
                    && mouse_event.row >= service_form_area.y
                    && mouse_event.row < service_form_area.y + service_form_area.height;

                if inside {
                    let mouse_x = mouse_event.column.saturating_sub(service_form_area.x) + scroll_x;
                    let mouse_y = mouse_event.row.saturating_sub(total_offset) + scroll_y;
            
                    let adjusted_event = MouseEvent {
                        kind: mouse_event.kind,
                        column: mouse_x,
                        row: mouse_y,
                        modifiers: mouse_event.modifiers,
                    };
            
                    log::info!(
                        "Mouse: ({}, {}), Adjusted: ({}, {}), Scroll: ({}, {}), Area: {:?}, GetKeysBtn: {:?}, TotalOffset: {}",
                        mouse_event.column, mouse_event.row, mouse_x, mouse_y, scroll_x, scroll_y, service_form_area,
                        self.get_keys_btn.get_area(), total_offset
                    );

                    self.get_keys_btn.handle_mouse_event(&adjusted_event);
                    self.check_seb_btn.handle_mouse_event(&adjusted_event);
                    self.webroot_key_btn.handle_mouse_event(&adjusted_event);
                    self.superanti_key_btn.handle_mouse_event(&adjusted_event);
                    self.get_ticket_btn.handle_mouse_event(&adjusted_event);
                    self.submit_btn.handle_mouse_event(&adjusted_event);
            
                    self.customer_name.handle_mouse_event(&adjusted_event);
                    self.customer_phone.handle_mouse_event(&adjusted_event);
                    self.salesman_name.handle_mouse_event(&adjusted_event);
                    self.technician_name.handle_mouse_event(&adjusted_event);
                    self.checkin_notes.handle_mouse_event(&adjusted_event);
                    self.recommendations.handle_mouse_event(&adjusted_event);
                    self.order_number.handle_mouse_event(&adjusted_event);
                }
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        let shift_pressed = key_event.modifiers.contains(KeyModifiers::SHIFT);
        if shift_pressed {
            if let KeyCode::Tab = key_event.code {
                log::info!("SHIFT Tab");
                let Some(ref active_field) = *self.active_field.borrow() else { return false; };
                let input_idx = Self::get_input_idx(&active_field);
                if input_idx > 0 {
                    self.set_input_idx(input_idx - 1);
                }
                true
            } else {
                false
            }
        } else {
            match key_event.code {
                KeyCode::Tab => {
                    // Get current index directly from input_idx instead of active_field
                    let input_idx = *self.input_idx.borrow();
                    self.set_input_state_from_input_idx(input_idx, ButtonState::Normal);
                    log::info!("active field: {:?} / input_idx: {}", self.active_field.borrow(), input_idx);
                    self.set_input_idx(input_idx + 1);
                    self.set_input_state_from_input_idx(input_idx + 1, ButtonState::Active);
                    true
                }
                KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    let text_area_input = self.order_number.input.borrow();
                    let user_input = &text_area_input.lines()[0];
                    log::info!("(Enter) 'Get Ticket' with input: {user_input}");
                    true
                }
                _ => {
                            // Dispatch key events to the active input field.
                    if let Some(ref active) = *self.active_field.borrow() {
                        match active.0.as_str() {
                            "CustomerName" => self.customer_name.input.borrow_mut().input_without_shortcuts(key_event),
                            "CustomerPhone" => self.customer_phone.input.borrow_mut().input_without_shortcuts(key_event),
                            "SalesmanName" => self.salesman_name.input.borrow_mut().input_without_shortcuts(key_event),
                            "TechnicianName" => self.technician_name.input.borrow_mut().input_without_shortcuts(key_event),
                            "CheckInNotes" => self.checkin_notes.input.borrow_mut().input_without_shortcuts(key_event),
                            "Recommendations" => self.recommendations.input.borrow_mut().input_without_shortcuts(key_event),
                            "ServiceNumber" => {
                                let order_num_field = &mut self.order_number;
                                let mut text_area_input = order_num_field.input.borrow_mut();
                                let input = text_area_input.input_without_shortcuts(key_event);
                                if input {
                                    if let Err(err) = text_area_input.lines()[0].parse::<i32>() {
                                        order_num_field.set_block(
                                            Block::default()
                                                .borders(Borders::ALL)
                                                .border_style(CATPPUCCIN.maroon)
                                                .border_set(SHORTCUT_SET)
                                                .title(format!("ERROR: {}", err)),
                                        );
                                        false
                                    } else {
                                        order_num_field.set_block(
                                            Block::default()
                                                .border_style(CATPPUCCIN.green)
                                                .borders(Borders::ALL)
                                                .border_set(SHORTCUT_SET)
                                                .title("Service #"),
                                        );
                                        true
                                    }
                                } else { false }
                            },
                            _ => {false}
                        }
                    } else { false }

                }
            }
        }
    }
}

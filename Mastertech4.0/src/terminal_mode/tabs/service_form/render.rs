use ratatui::{crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent}, layout::{Constraint, Direction, Layout, Position, Rect, Size}, prelude::{Backend, StatefulWidget}, style::{Color, Style}, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::terminal_mode::{styling::{CATPPUCCIN, APP_BACKGROUND}, widgets::{button::ButtonState, ButtonType, HandleWidget, ShrinkArea, SHORTCUT_SET}};
use crate::terminal_mode::widgets::tui_scroll_view::ScrollView;
use super::ServiceFormTab;

// Define a virtual height for the service form content.
pub const SERVICE_FORM_VIRTUAL_HEIGHT: u16 = 46;

/// Implement the HandleWidget trait for ServiceFormTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> HandleWidget<'a> for ServiceFormTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {

        let total_offset = area.y;
        self.total_offset.replace(total_offset);
        self.service_form_area.replace(Some(area));

        let virtual_size = Size {
            width: area.width,
            height: SERVICE_FORM_VIRTUAL_HEIGHT,
        };

        let mut scroll_view = ScrollView::new(virtual_size);
        scroll_view.buf_mut().set_style(area, Style::new().bg(APP_BACKGROUND));
        
        // Calculate centered content area (60% of total width, centered)
        let content_width_percent = 60;
        let side_margin_percent = (100 - content_width_percent) / 2;
        
        // Outer layout to center the content horizontally
        let centered_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(side_margin_percent as u16),  // Left margin
                Constraint::Percentage(content_width_percent as u16), // Centered content
                Constraint::Percentage(side_margin_percent as u16),  // Right margin
            ])
            .split(area);
        
        let content_area = centered_layout[1];
        
        // Updated constraints: Compact layout without product rows
        let constraints = vec![
            Constraint::Length(3),  // Row 1: Get Ticket | Submit
            Constraint::Length(3),  // Row 2: Service Number | Customer Email
            Constraint::Length(3),  // Row 3: Customer Name | Customer Phone | Device Name | Device Mfg
            Constraint::Length(3),  // Row 4: Salesman | Technician | Device Model | Device Serial
            Constraint::Length(3),  // Row 5: Get Keys | Check SEB | Device Password | Device Power
            Constraint::Length(3),  // Row 6: Carbonite Name | Device ID | Activation Code | Recurly
            Constraint::Length(3),  // Row 7: Webroot Key | SuperAnti Key
            Constraint::Length(8),  // Row 8: CheckIn Notes | Recommendations
            Constraint::Min(1),     // Spacer
        ];

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints.as_slice())
            .split(content_area);

        // Row 1: Get Ticket | Submit - buttons centered
        let row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),  // Get Ticket
                Constraint::Percentage(25),  // Submit
                Constraint::Percentage(50),  // Spacer
            ])
            .split(rows[0]);
        let get_ticket_btn_area = row1[0].shrink(2, 0);
        self.get_ticket_btn.render_ref(get_ticket_btn_area, scroll_view.buf_mut());
        let submit_btn_area = row1[1].shrink(2, 0);
        self.submit_btn.render_ref(submit_btn_area, scroll_view.buf_mut());

        // Row 2: Service Number | Customer Email (50% each in content area)
        let row2 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),  // Service Number
                Constraint::Percentage(50),  // Customer Email
            ])
            .split(rows[1]);
        self.order_number.render_ref(row2[0], scroll_view.buf_mut());
        if !self.other_fields.is_empty() {
            self.other_fields[0].render_ref(row2[1], scroll_view.buf_mut()); // Customer Email
        }

        // Row 3: Customer Name | Customer Phone | Device Name | Device Mfg (25% each)
        let row3 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[2]);
        self.customer_name.render_ref(row3[0], scroll_view.buf_mut());
        self.customer_phone.render_ref(row3[1], scroll_view.buf_mut());
        if self.other_fields.len() > 1 {
            self.other_fields[1].render_ref(row3[2], scroll_view.buf_mut()); // Device Name
        }
        if self.other_fields.len() > 2 {
            self.other_fields[2].render_ref(row3[3], scroll_view.buf_mut()); // Device Mfg
        }

        // Row 4: Salesman | Technician | Device Model | Device Serial (25% each)
        let row4 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[3]);

        let salesman = &self.salesman_name;
        let tech = &self.technician_name;
        salesman.set_total_offset(total_offset);
        tech.set_total_offset(total_offset);

        if let (Ok(mut suggestions), Ok(mut tech_suggestions)) = (salesman.suggestions.try_borrow_mut(), tech.suggestions.try_borrow_mut()) {
            if suggestions.is_empty() {
                if let Ok(ctx) = self.ctx.try_lock() {
                    if !ctx.store_users.is_empty() {
                        let users = ctx.store_users.iter().filter(|u| u.get_store() == ctx.user.get_store()).map(|u| u.get_username().to_string()).collect::<Vec<String>>();
                        *tech_suggestions = users.clone();
                        *suggestions = users;
                    }
                }
            }
        }

        salesman.render_ref(row4[0], scroll_view.buf_mut());
        salesman.set_on_screen_area(row4[0]);
        tech.render_ref(row4[1], scroll_view.buf_mut());
        tech.set_on_screen_area(row4[1]);
        if self.other_fields.len() > 3 {
            self.other_fields[3].render_ref(row4[2], scroll_view.buf_mut()); // Device Model
        }
        if self.other_fields.len() > 4 {
            self.other_fields[4].render_ref(row4[3], scroll_view.buf_mut()); // Device Serial
        }

        // Row 5: Get Keys | Check SEB | Device Password | Device Power (25% each)
        let row5 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[4]);
        let get_keys_btn_area = row5[0].shrink(2, 0);
        self.get_keys_btn.render_ref(get_keys_btn_area, scroll_view.buf_mut());
        let check_seb_btn_area = row5[1].shrink(2, 0);
        self.check_seb_btn.render_ref(check_seb_btn_area, scroll_view.buf_mut());
        if self.other_fields.len() > 5 {
            self.other_fields[5].render_ref(row5[2], scroll_view.buf_mut()); // Device Password
        }
        if self.other_fields.len() > 6 {
            self.other_fields[6].render_ref(row5[3], scroll_view.buf_mut()); // Device Powersupply
        }

        // Row 6: Carbonite Name | Device ID | Activation Code | Recurly (25% each)
        let row6 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[5]);
        if !self.seb_fields.is_empty() {
            self.seb_fields[0].render_ref(row6[0], scroll_view.buf_mut()); // Carbonite Device Name
        }
        if self.seb_fields.len() > 1 {
            self.seb_fields[1].render_ref(row6[1], scroll_view.buf_mut()); // Device ID
        }
        if self.seb_fields.len() > 2 {
            self.seb_fields[2].render_ref(row6[2], scroll_view.buf_mut()); // Activation Code
        }
        if self.seb_fields.len() > 3 {
            self.seb_fields[3].render_ref(row6[3], scroll_view.buf_mut()); // Recurly Id
        }

        // Row 7: Webroot Key | SuperAnti Key (50% each - stays wide for long keys)
        let row7 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),  // Webroot Key - wide for long keys
                Constraint::Percentage(50),  // SuperAnti Key - wide for long keys
            ])
            .split(rows[6]);
        let webroot_key_btn_area = row7[0].shrink(2, 0);
        self.webroot_key_btn.render_ref(webroot_key_btn_area, scroll_view.buf_mut());
        let superanti_key_btn_area = row7[1].shrink(2, 0);
        self.superanti_key_btn.render_ref(superanti_key_btn_area, scroll_view.buf_mut());

        // Row 8: CheckIn Notes | Recommendations (50% each)
        let row8 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),  // CheckIn Notes
                Constraint::Percentage(50),  // Recommendations
            ])
            .split(rows[7]);
        self.checkin_notes.render_ref(row8[0], scroll_view.buf_mut());
        self.recommendations.render_ref(row8[1], scroll_view.buf_mut());

        // No more product rows - removed

        scroll_view.render(area, f.buffer_mut(), &mut self.scroll_state.borrow_mut());

        let s_state = if let Ok(s_state) = self.scroll_state.try_borrow() {
            s_state.offset()
        } else {
            Position::default()
        };

        // Set cursor position for the active field
        if let Some(ref active) = *self.active_field.borrow() {
            let cursor_pos = match active.0.as_str() {
                "CustomerName" => self.customer_name.get_cursor_position(),
                "CustomerPhone" => self.customer_phone.get_cursor_position(),
                "SalesmanName" => self.salesman_name.get_cursor_position(),
                "TechnicianName" => self.technician_name.get_cursor_position(),
                "CheckInNotes" => self.checkin_notes.get_cursor_position(),
                "Recommendations" => self.recommendations.get_cursor_position(),
                "ServiceNumber" => self.order_number.get_cursor_position(),
                _ => None,
            };
            if let Some(pos) = cursor_pos {
                let scroll_y = s_state.y;
                let total_offset = *self.total_offset.borrow();
                // Adjust y with total_offset and scroll_y
                let adjusted_y = pos.y + total_offset - scroll_y;
                if adjusted_y >= area.y && adjusted_y < area.y + area.height {
                    f.set_cursor_position(Position::new(pos.x, adjusted_y));
                }
            }
        }
        salesman.render_popup(f.buffer_mut());
        tech.render_popup(f.buffer_mut());

        // Draw the duplicate merge modal if it's open
        if let Some(ref modal) = *self.duplicate_modal.borrow() {
            modal.draw(f, area);
        }
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // Handle modal mouse events first if modal is open
        if let Ok(mut modal_opt) = self.duplicate_modal.try_borrow_mut() {
            if let Some(ref mut modal) = *modal_opt {
                if modal.is_open {
                    if modal.handle_mouse_event(mouse_event) {
                        // Check if modal was confirmed or cancelled
                        if modal.is_confirmed() {
                            log::info!("Modal confirmed, submitting with resolution");
                            let resolution = Some(modal.get_resolution().clone());
                            drop(modal_opt); // Release borrow before accessing ctx
                            if let Ok(mut ctx) = self.ctx.try_lock() {
                                ctx.service_data.submit_after_resolution(resolution);
                            }
                            self.duplicate_modal.replace(None);
                        } else if modal.is_cancelled() {
                            log::info!("Modal cancelled");
                            drop(modal_opt);
                            self.duplicate_modal.replace(None);
                        }
                        return;
                    }
                    return; // Modal is open, consume event
                }
            }
        }

        // Handle popup mouse events first, as they are on top
        if self.salesman_name.handle_popup_mouse(mouse_event) {
            return;
        }
        if self.technician_name.handle_popup_mouse(mouse_event) {
            return;
        }

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
                let total_offset = *self.total_offset.borrow();

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

                    // log::info!(
                    //     "Mouse: ({}, {}), Adjusted: ({}, {}), Scroll: ({}, {}), TotalOffset: {}",
                    //     mouse_event.column, mouse_event.row, mouse_x, mouse_y, scroll_x, scroll_y, total_offset
                    // );
                    for input_fields in self.other_fields.iter() {
                        input_fields.handle_mouse_event(&adjusted_event);
                    }

                    for input_fields in self.seb_fields.iter() {
                        input_fields.handle_mouse_event(&adjusted_event);
                    }

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
        // Handle modal key events first if modal is open
        if let Ok(mut modal_opt) = self.duplicate_modal.try_borrow_mut() {
            if let Some(ref mut modal) = *modal_opt {
                if modal.is_open {
                    if modal.handle_key_event(key_event) {
                        // Check if modal was confirmed or cancelled
                        if modal.is_confirmed() {
                            log::info!("Modal confirmed via keyboard, submitting with resolution");
                            let resolution = Some(modal.get_resolution().clone());
                            drop(modal_opt); // Release borrow before accessing ctx
                            if let Ok(mut ctx) = self.ctx.try_lock() {
                                ctx.service_data.submit_after_resolution(resolution);
                            }
                            self.duplicate_modal.replace(None);
                        } else if modal.is_cancelled() {
                            log::info!("Modal cancelled via keyboard");
                            drop(modal_opt);
                            self.duplicate_modal.replace(None);
                        }
                        return true;
                    }
                    return true; // Modal is open, consume event
                }
            }
        }

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
                if let Some(ref active) = *self.active_field.borrow() {
                    match active.0.as_str() {
                        "CustomerName" => self.customer_name.handle_key_event(&key_event),
                        "CustomerPhone" => self.customer_phone.handle_key_event(&key_event),
                        "SalesmanName" => self.salesman_name.handle_key_event(&key_event),
                        "TechnicianName" => self.technician_name.handle_key_event(&key_event),
                        "CheckInNotes" => self.checkin_notes.handle_key_event(&key_event),
                        "Recommendations" => self.recommendations.handle_key_event(&key_event),
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
                                    
                                } else {
                                    order_num_field.set_block(
                                        Block::default()
                                            .border_style(CATPPUCCIN.green)
                                            .borders(Borders::ALL)
                                            .border_set(SHORTCUT_SET)
                                            .title("Service #"),
                                    );
                                    
                                }
                            }
                            false
                        },
                        _ => {false}
                    };
                }
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
                    let user_input = &self.order_number.get_text()[0];
                    log::info!("(Enter) 'Get Ticket' with input: {user_input}");
                    true
                }
                _ => {

                    // Dispatch key events to the active input field.
                    if let Some(ref active) = *self.active_field.borrow() {
                        match active.0.as_str() {
                            "CustomerName" => self.customer_name.handle_key_event(&key_event),
                            "CustomerPhone" => self.customer_phone.handle_key_event(&key_event),
                            "SalesmanName" => self.salesman_name.handle_key_event(&key_event),
                            "TechnicianName" => self.technician_name.handle_key_event(&key_event),
                            "CheckInNotes" => self.checkin_notes.handle_key_event(&key_event),
                            "Recommendations" => self.recommendations.handle_key_event(&key_event),
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

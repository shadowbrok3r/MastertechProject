//! Rendering implementation for TaskModal

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Backend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use database::schema::Status;
use crate::terminal_mode::{
    styling::CATPPUCCIN,
    widgets::{ButtonType, HandleWidget},
};
use super::{ModalPage, TaskModal};

impl<'a> HandleWidget<'a> for TaskModal<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let modal_area = self.calculate_modal_area(area);
        *self.modal_area.borrow_mut() = modal_area;
        
        // Draw dimmed background
        let dim_block = Block::default()
            .style(Style::default().bg(Color::Rgb(0, 0, 0)));
        f.render_widget(Clear, area);
        f.render_widget(dim_block, area);
        
        // Draw modal background
        let modal_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CATPPUCCIN.lavender))
            .style(Style::default().bg(Color::Rgb(20, 20, 28)))
            .title(format!(" {} - {} ", self.task.task_name, self.modal_id))
            .title_alignment(ratatui::layout::Alignment::Center);
        
        f.render_widget(modal_block.clone(), modal_area);
        
        let inner_area = modal_block.inner(modal_area);
        
        // Layout: Header with tabs, then content
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Length(1), // Separator
                Constraint::Min(1),    // Content
                Constraint::Length(2), // Footer with close hint
            ])
            .split(inner_area);
        
        // Draw tab buttons
        *self.tab_bar_area.borrow_mut() = layout[0];
        self.draw_tab_buttons(f, layout[0]);
        
        // Draw content based on current page
        let current_page = *self.current_page.borrow();
        match current_page {
            ModalPage::TicketInfo => self.draw_ticket_page(f, layout[2]),
            ModalPage::ComputerInfo => self.draw_computer_page(f, layout[2]),
            ModalPage::SoftwareInfo => self.draw_software_page(f, layout[2]),
            ModalPage::TaskHistory => self.draw_history_page(f, layout[2]),
            ModalPage::TaskNotes => self.draw_notes_page(f, layout[2]),
        }
        
        // Draw footer
        let footer = Paragraph::new(Line::from(vec![
            Span::styled("ESC", Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)),
            Span::raw(" Close  "),
            Span::styled("Tab", Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)),
            Span::raw(" Switch Tab  "),
            Span::styled("↑↓", Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)),
            Span::raw(" Scroll"),
        ]))
        .alignment(ratatui::layout::Alignment::Center)
        .style(Style::default().fg(CATPPUCCIN.subtext0));
        f.render_widget(footer, layout[3]);
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let modal_area = *self.modal_area.borrow();
        let x = mouse_event.column;
        let y = mouse_event.row;
        
        // Check if click is inside modal
        let inside_modal = x >= modal_area.x && x < modal_area.right()
            && y >= modal_area.y && y < modal_area.bottom();
        
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !inside_modal {
                    // Click outside modal closes it
                    self.request_close();
                    return;
                }
                
                // Forward to tab buttons
                for btn in &self.tab_buttons {
                    btn.handle_mouse_event(mouse_event);
                }
                
                // Forward to close button
                self.close_btn.handle_mouse_event(mouse_event);
            }
            MouseEventKind::Moved => {
                // Forward hover events to buttons
                for btn in &self.tab_buttons {
                    btn.handle_mouse_event(mouse_event);
                }
                self.close_btn.handle_mouse_event(mouse_event);
            }
            MouseEventKind::ScrollDown => {
                *self.scroll_offset.borrow_mut() += 1;
            }
            MouseEventKind::ScrollUp => {
                let mut offset = self.scroll_offset.borrow_mut();
                *offset = offset.saturating_sub(1);
            }
            _ => {
                // Forward other events
                for btn in &self.tab_buttons {
                    btn.handle_mouse_event(mouse_event);
                }
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.request_close();
                true
            }
            KeyCode::Tab => {
                let current = *self.current_page.borrow();
                let next_idx = (current.index() + 1) % ModalPage::all().len();
                self.set_active_tab(ModalPage::from_index(next_idx));
                true
            }
            KeyCode::BackTab => {
                let current = *self.current_page.borrow();
                let prev_idx = if current.index() == 0 {
                    ModalPage::all().len() - 1
                } else {
                    current.index() - 1
                };
                self.set_active_tab(ModalPage::from_index(prev_idx));
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let mut offset = self.scroll_offset.borrow_mut();
                *offset = offset.saturating_sub(1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                *self.scroll_offset.borrow_mut() += 1;
                true
            }
            KeyCode::Char('1') => {
                self.set_active_tab(ModalPage::TicketInfo);
                true
            }
            KeyCode::Char('2') => {
                self.set_active_tab(ModalPage::ComputerInfo);
                true
            }
            KeyCode::Char('3') => {
                self.set_active_tab(ModalPage::SoftwareInfo);
                true
            }
            KeyCode::Char('4') => {
                self.set_active_tab(ModalPage::TaskHistory);
                true
            }
            KeyCode::Char('5') => {
                self.set_active_tab(ModalPage::TaskNotes);
                true
            }
            _ => false
        }
    }
}

impl<'a> TaskModal<'a> {
    /// Calculate the modal area centered in the given area
    pub(crate) fn calculate_modal_area(&self, area: Rect) -> Rect {
        let modal_width = (area.width as f32 * 0.85).min(120.0) as u16;
        let modal_height = (area.height as f32 * 0.85).min(50.0) as u16;
        
        let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
        let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
        
        Rect::new(x, y, modal_width, modal_height)
    }
    
    /// Draw tab buttons in the tab bar area
    pub(crate) fn draw_tab_buttons(&mut self, f: &mut Frame, area: Rect) {
        let num_tabs = self.tab_buttons.len();
        let constraints: Vec<Constraint> = (0..num_tabs)
            .map(|_| Constraint::Ratio(1, num_tabs as u32))
            .collect();
        
        let tab_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);
        
        // Store tab areas for click detection and update button areas
        *self.tab_button_areas.borrow_mut() = tab_areas.to_vec();
        
        // Draw each tab button using render_widget
        for (i, btn) in self.tab_buttons.iter().enumerate() {
            // Update button's stored area for mouse detection
            btn.set_area(tab_areas[i]);
            f.render_widget(btn, tab_areas[i]);
        }
    }
    
    pub(crate) fn draw_ticket_page(&self, f: &mut Frame, area: Rect) {
        let task = &self.task;
        let ticket = self.ticket.borrow();
        let customer = self.customer.borrow();
        
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12), // Task info grid
                Constraint::Length(1),  // Separator
                Constraint::Min(1),     // Description/notes
            ])
            .split(area);
        
        // Task info section
        let info_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[0]);
        
        // Left column
        let mut left_lines = vec![
            Line::from(vec![
                Span::styled("Service #: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(task.service_number.clone().unwrap_or_default()),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(CATPPUCCIN.red)),
                Span::styled(task.status.as_str(), Self::status_style(&task.status)),
            ]),
            Line::from(vec![
                Span::styled("Priority: ", Style::default().fg(CATPPUCCIN.red)),
                Span::styled(task.priority.as_str(), Self::priority_style(&task.priority)),
            ]),
            Line::from(vec![
                Span::styled("Due Date: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(task.due_date.format("%m/%d/%Y").to_string()),
            ]),
            Line::from(vec![
                Span::styled("Completed: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(if task.completed { "Yes" } else { "No" }),
            ]),
        ];
        
        if let Some(ref ticket) = *ticket {
            left_lines.push(Line::from(vec![
                Span::styled("Tech: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(&ticket.tech),
            ]));
            left_lines.push(Line::from(vec![
                Span::styled("Salesman: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(&ticket.salesman),
            ]));
        }
        
        let left_para = Paragraph::new(left_lines)
            .block(Block::default().borders(Borders::RIGHT).border_style(Style::default().fg(CATPPUCCIN.surface0)));
        f.render_widget(left_para, info_layout[0]);
        
        // Right column - Customer info
        let mut right_lines = vec![];
        if let Some(ref cust) = *customer {
            right_lines.push(Line::from(vec![
                Span::styled("Customer: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(&cust.name),
            ]));
            right_lines.push(Line::from(vec![
                Span::styled("Phone: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(&cust.phone_number),
            ]));
            right_lines.push(Line::from(vec![
                Span::styled("Email: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(&cust.email),
            ]));
            right_lines.push(Line::from(vec![
                Span::styled("Cust Code: ", Style::default().fg(CATPPUCCIN.red)),
                Span::raw(&cust.cust_code),
            ]));
        } else {
            right_lines.push(Line::styled("Loading customer data...", Style::default().fg(CATPPUCCIN.subtext0)));
        }
        
        let right_para = Paragraph::new(right_lines);
        f.render_widget(right_para, info_layout[1]);
        
        // Description section
        let desc_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(CATPPUCCIN.surface0))
            .title(" Description ")
            .title_style(Style::default().fg(CATPPUCCIN.peach));
        
        let desc_para = Paragraph::new(task.task_description.clone())
            .block(desc_block)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(CATPPUCCIN.text));
        
        f.render_widget(desc_para, layout[2]);
    }
    
    pub(crate) fn draw_computer_page(&self, f: &mut Frame, area: Rect) {
        let computer = self.computer.borrow();
        
        let lines: Vec<Line> = if let Some(ref comp) = *computer {
            vec![
                Line::from(vec![
                    Span::styled("Hostname: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.hostname),
                ]),
                Line::from(vec![
                    Span::styled("CPU: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.cpu),
                ]),
                Line::from(vec![
                    Span::styled("GPU: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.gpu),
                ]),
                Line::from(vec![
                    Span::styled("RAM: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.ram),
                ]),
                Line::from(vec![
                    Span::styled("OS: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.operating_system),
                ]),
                Line::from(vec![
                    Span::styled("Device Name: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(comp.device_name.as_deref().unwrap_or("N/A")),
                ]),
                Line::from(vec![
                    Span::styled("Device Mfg: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(comp.device_mfg.as_deref().unwrap_or("N/A")),
                ]),
                Line::from(vec![
                    Span::styled("Device Model: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(comp.device_model.as_deref().unwrap_or("N/A")),
                ]),
                Line::from(vec![
                    Span::styled("Device Serial: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(comp.device_serial.as_deref().unwrap_or("N/A")),
                ]),
                Line::from(vec![
                    Span::styled("Product Name: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.product_name),
                ]),
                Line::from(vec![
                    Span::styled("Product Vendor: ", Style::default().fg(CATPPUCCIN.red)),
                    Span::raw(&comp.product_vendor),
                ]),
            ]
        } else {
            vec![Line::styled("Loading computer data...", Style::default().fg(CATPPUCCIN.subtext0))]
        };
        
        let para = Paragraph::new(lines)
            .block(Block::default().title(" Computer Info ").title_style(Style::default().fg(CATPPUCCIN.peach)));
        f.render_widget(para, area);
    }
    
    pub(crate) fn draw_software_page(&self, f: &mut Frame, area: Rect) {
        let computer = self.computer.borrow();
        
        let lines: Vec<Line> = if let Some(ref comp) = *computer {
            let mut lines = vec![
                Line::styled("Installed Software:", Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)),
                Line::raw(""),
            ];
            
            // Parse installed_programs JSON if available
            if let Some(ref programs) = comp.installed_programs {
                if let Some(arr) = programs.as_array() {
                    for program in arr.iter().take(20) { // Limit to first 20
                        if let Some(name) = program.get("name").and_then(|n| n.as_str()) {
                            let version = program.get("version").and_then(|v| v.as_str()).unwrap_or("");
                            lines.push(Line::from(vec![
                                Span::styled("• ", Style::default().fg(CATPPUCCIN.green)),
                                Span::raw(format!("{} {}", name, version)),
                            ]));
                        }
                    }
                }
            }
            
            if lines.len() <= 2 {
                lines.push(Line::styled("No software data available", Style::default().fg(CATPPUCCIN.subtext0)));
            }
            
            lines
        } else {
            vec![Line::styled("Loading software data...", Style::default().fg(CATPPUCCIN.subtext0))]
        };
        
        let para = Paragraph::new(lines)
            .block(Block::default().title(" Software ").title_style(Style::default().fg(CATPPUCCIN.peach)));
        f.render_widget(para, area);
    }
    
    pub(crate) fn draw_history_page(&self, f: &mut Frame, area: Rect) {
        let history = self.history.borrow();
        
        let lines: Vec<Line> = if history.is_empty() {
            vec![Line::styled("No history available", Style::default().fg(CATPPUCCIN.subtext0))]
        } else {
            history.iter().flat_map(|h| {
                let created: chrono::DateTime<chrono::Utc> = h.created_at.clone().into();
                // Format diff for display
                let diff_str = if let Some(obj) = h.diff.as_object() {
                    obj.iter()
                        .map(|(k, v)| {
                            let old = v.get("old").and_then(|o| o.as_str()).unwrap_or("?");
                            let new = v.get("new").and_then(|n| n.as_str()).unwrap_or("?");
                            format!("{}: {} → {}", k, old, new)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    h.diff.to_string()
                };
                
                vec![
                    Line::from(vec![
                        Span::styled(created.format("%m/%d/%y %H:%M").to_string(), Style::default().fg(CATPPUCCIN.blue)),
                        Span::raw(" - "),
                        Span::styled(&h.username, Style::default().fg(CATPPUCCIN.peach)),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::raw(diff_str),
                    ]),
                    Line::raw(""),
                ]
            }).collect()
        };
        
        let para = Paragraph::new(lines)
            .block(Block::default().title(" Task History ").title_style(Style::default().fg(CATPPUCCIN.peach)));
        f.render_widget(para, area);
    }
    
    pub(crate) fn draw_notes_page(&self, f: &mut Frame, area: Rect) {
        let notes = self.notes.borrow();
        
        let lines: Vec<Line> = if notes.is_empty() {
            vec![Line::styled("No notes available", Style::default().fg(CATPPUCCIN.subtext0))]
        } else {
            notes.iter().flat_map(|n| {
                let created: chrono::DateTime<chrono::Utc> = n.created_at.clone().into();
                vec![
                    Line::from(vec![
                        Span::styled(&n.username, Style::default().fg(CATPPUCCIN.blue).add_modifier(Modifier::BOLD)),
                        Span::raw(" - "),
                        Span::styled(created.format("%m/%d/%y %H:%M").to_string(), Style::default().fg(CATPPUCCIN.subtext0)),
                    ]),
                    Line::from(Span::raw(&n.note)),
                    Line::raw(""),
                ]
            }).collect()
        };
        
        let para = Paragraph::new(lines)
            .block(Block::default().title(" Task Notes ").title_style(Style::default().fg(CATPPUCCIN.peach)))
            .wrap(Wrap { trim: true });
        f.render_widget(para, area);
    }
    
    fn status_style(status: &Status) -> Style {
        let color = match status {
            Status::Todo => CATPPUCCIN.yellow,
            Status::InRepair => CATPPUCCIN.blue,
            Status::Complete => CATPPUCCIN.green,
            Status::Qc => CATPPUCCIN.mauve,
            Status::Sales => CATPPUCCIN.peach,
            Status::CustomStatus(_) => CATPPUCCIN.text,
        };
        Style::default().fg(color)
    }
    
    fn priority_style(priority: &database::schema::Priority) -> Style {
        let color = match priority {
            database::schema::Priority::Express => CATPPUCCIN.red,
            database::schema::Priority::Fire => CATPPUCCIN.maroon,
            database::schema::Priority::Rfs => CATPPUCCIN.peach,
            database::schema::Priority::Qc => CATPPUCCIN.mauve,
            database::schema::Priority::Normal => CATPPUCCIN.text,
        };
        Style::default().fg(color)
    }
}

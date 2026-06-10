//! Rendering implementation for TaskModal

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    prelude::Backend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};
use database::schema::{Status, TaskNotePayload};
use unicode_width::UnicodeWidthStr;
use crate::terminal_mode::{
    styling::{CATPPUCCIN, THEME, APP_BACKGROUND},
    widgets::{tui_textarea::CursorMove, ButtonType, HandleWidget},
};
use super::{ModalFocus, ModalPage, TaskModal};

impl<'a> HandleWidget<'a> for TaskModal<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.receive();
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
            .border_style(Style::default().fg(THEME.accent))
            .title_style(THEME.title())
            .style(Style::default().bg(APP_BACKGROUND))
            .title(format!(" {} - {} ", self.task.task_name, self.modal_id))
            .title_alignment(Alignment::Center);

        f.render_widget(modal_block.clone(), modal_area);

        let inner_area = modal_block.inner(modal_area);

        // Layout: Header with tabs, then content
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Length(1), // Separator
                Constraint::Min(1),    // Content
                Constraint::Length(2), // Footer with key hints
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

        // Field selector popup paints on top of everything.
        self.draw_selector_popup(f, modal_area);

        // Draw footer
        self.draw_footer(f, layout[3]);
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let modal_area = *self.modal_area.borrow();
        let x = mouse_event.column;
        let y = mouse_event.row;

        // Check if click is inside modal
        let inside_modal = x >= modal_area.x && x < modal_area.right()
            && y >= modal_area.y && y < modal_area.bottom();

        // Forward to tab buttons
        for btn in &self.tab_buttons {
            btn.handle_mouse_event(mouse_event);
        }

        // Forward to close button
        self.close_btn.handle_mouse_event(mouse_event);

        let on_notes_page = *self.current_page.borrow() == ModalPage::TaskNotes;

        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let pos = Position::new(x, y);
                // Switch tabs on click.
                let tab_areas = self.tab_button_areas.borrow().clone();
                if let Some(i) = tab_areas.iter().position(|ta| ta.contains(pos)) {
                    *self.current_page.borrow_mut() = ModalPage::from_index(i);
                    *self.focus.borrow_mut() = ModalFocus::Normal;
                    for (j, btn) in self.tab_buttons.iter().enumerate() {
                        btn.set_selected(j == i);
                    }
                    return;
                }
                // Click into the note input focuses it.
                if on_notes_page && self.note_input_area.borrow().contains(pos) {
                    *self.focus.borrow_mut() = ModalFocus::NoteInput;
                    return;
                }
                // Click elsewhere drops note-input focus.
                if *self.focus.borrow() == ModalFocus::NoteInput {
                    *self.focus.borrow_mut() = ModalFocus::Normal;
                }
                if !inside_modal {
                    // Click outside modal closes it
                    self.request_close();
                    return;
                }
            }
            MouseEventKind::ScrollDown => {
                if on_notes_page {
                    let mut scroll = self.notes_scroll.borrow_mut();
                    *scroll = scroll.saturating_sub(1);
                } else {
                    *self.scroll_offset.borrow_mut() += 1;
                }
            }
            MouseEventKind::ScrollUp => {
                if on_notes_page {
                    *self.notes_scroll.borrow_mut() += 1;
                } else {
                    let mut offset = self.scroll_offset.borrow_mut();
                    *offset = offset.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        let focus = *self.focus.borrow();
        match focus {
            ModalFocus::Selector => {
                match key_event.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Some(selector) = self.selector.as_mut() {
                            selector.select_prev();
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(selector) = self.selector.as_mut() {
                            selector.select_next();
                        }
                    }
                    KeyCode::Enter => self.apply_selector(),
                    KeyCode::Esc | KeyCode::Char('q') => self.cancel_selector(),
                    _ => {}
                }
                true
            }
            ModalFocus::EditDescription => {
                match key_event.code {
                    KeyCode::Esc => {
                        *self.focus.borrow_mut() = ModalFocus::Normal;
                        self.set_status("Description edit cancelled");
                    }
                    KeyCode::Char('s') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.save_description();
                    }
                    _ => {
                        Self::feed_editor_key(&mut self.description_editor, &key_event);
                    }
                }
                true
            }
            ModalFocus::NoteInput => {
                match key_event.code {
                    KeyCode::Esc => {
                        *self.focus.borrow_mut() = ModalFocus::Normal;
                    }
                    KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::ALT) => {
                        self.note_editor.insert_newline();
                    }
                    KeyCode::Enter => self.send_note(),
                    KeyCode::Char('p') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.note_private = !self.note_private;
                    }
                    _ => {
                        Self::feed_editor_key(&mut self.note_editor, &key_event);
                    }
                }
                true
            }
            ModalFocus::Normal => self.handle_normal_key(key_event),
        }
    }
}

impl<'a> TaskModal<'a> {
    /// Key handling when no editor or popup has focus.
    fn handle_normal_key(&mut self, key_event: KeyEvent) -> bool {
        let page = *self.current_page.borrow();
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.request_close();
                true
            }
            KeyCode::Tab => {
                let next_idx = (page.index() + 1) % ModalPage::all().len();
                self.set_active_tab(ModalPage::from_index(next_idx));
                true
            }
            KeyCode::BackTab => {
                let prev_idx = if page.index() == 0 {
                    ModalPage::all().len() - 1
                } else {
                    page.index() - 1
                };
                self.set_active_tab(ModalPage::from_index(prev_idx));
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if page == ModalPage::TaskNotes {
                    *self.notes_scroll.borrow_mut() += 1;
                } else {
                    let mut offset = self.scroll_offset.borrow_mut();
                    *offset = offset.saturating_sub(1);
                }
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if page == ModalPage::TaskNotes {
                    let mut scroll = self.notes_scroll.borrow_mut();
                    *scroll = scroll.saturating_sub(1);
                } else {
                    *self.scroll_offset.borrow_mut() += 1;
                }
                true
            }
            KeyCode::Char('e') if page == ModalPage::TicketInfo => {
                self.start_description_edit();
                true
            }
            KeyCode::Char('s') => {
                self.open_status_selector();
                true
            }
            KeyCode::Char('a') => {
                self.open_assignee_selector();
                true
            }
            KeyCode::Char('p') => {
                self.open_priority_selector();
                true
            }
            KeyCode::Char('c') => {
                self.toggle_completed();
                true
            }
            KeyCode::Char('i') | KeyCode::Enter if page == ModalPage::TaskNotes => {
                *self.focus.borrow_mut() = ModalFocus::NoteInput;
                true
            }
            KeyCode::Char('r') if page == ModalPage::TaskNotes => {
                self.refresh_notes();
                true
            }
            _ => false,
        }
    }

    /// Forward a key event to a textarea, mirroring InputField's bindings.
    fn feed_editor_key(editor: &mut crate::terminal_mode::widgets::tui_textarea::TextArea<'static>, key_event: &KeyEvent) {
        let modifiers = key_event.modifiers;
        match key_event.code {
            KeyCode::End => editor.move_cursor(CursorMove::End),
            KeyCode::Home => editor.move_cursor(CursorMove::Head),
            KeyCode::Up => editor.move_cursor(CursorMove::Up),
            KeyCode::Down => editor.move_cursor(CursorMove::Down),
            KeyCode::Left => editor.move_cursor(CursorMove::Back),
            KeyCode::Right => editor.move_cursor(CursorMove::Forward),
            KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => editor.select_all(),
            KeyCode::Char('z') if modifiers.contains(KeyModifiers::CONTROL) => { editor.undo(); }
            KeyCode::Char('y') if modifiers.contains(KeyModifiers::CONTROL) => { editor.redo(); }
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                if editor.selection_range().is_none() {
                    editor.select_all();
                }
                editor.copy();
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let yank_text = editor.yank_text();
                    if !yank_text.is_empty() {
                        let _ = clipboard.set().text(&yank_text);
                    }
                }
            }
            KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    if let Ok(contents) = clipboard.get().text() {
                        editor.insert_str(contents);
                    }
                }
            }
            _ => {
                editor.input_without_shortcuts(*key_event);
            }
        }
    }

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

    /// Context-sensitive footer key hints plus transient status feedback.
    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let key = |s: &'static str| Span::styled(s, Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD));
        let txt = |s: &'static str| Span::raw(s);

        let hints = match *self.focus.borrow() {
            ModalFocus::Selector => vec![
                key("↑↓"), txt(" Select  "),
                key("Enter"), txt(" Apply  "),
                key("Esc"), txt(" Cancel"),
            ],
            ModalFocus::EditDescription => vec![
                key("Ctrl+S"), txt(" Save  "),
                key("Esc"), txt(" Cancel"),
            ],
            ModalFocus::NoteInput => vec![
                key("Enter"), txt(" Send  "),
                key("Alt+Enter"), txt(" Newline  "),
                key("Ctrl+P"), txt(" Private  "),
                key("Esc"), txt(" Done"),
            ],
            ModalFocus::Normal => {
                let mut hints = vec![
                    key("Esc"), txt(" Close  "),
                    key("Tab"), txt(" Pages  "),
                    key("s"), txt(" Status  "),
                    key("a"), txt(" Assignee  "),
                    key("p"), txt(" Priority  "),
                    key("c"), txt(" Complete  "),
                ];
                match *self.current_page.borrow() {
                    ModalPage::TicketInfo => {
                        hints.push(key("e"));
                        hints.push(txt(" Edit Desc  "));
                    }
                    ModalPage::TaskNotes => {
                        hints.push(key("i"));
                        hints.push(txt(" Write Note  "));
                        hints.push(key("r"));
                        hints.push(txt(" Refresh  "));
                    }
                    _ => {}
                }
                hints.push(key("↑↓"));
                hints.push(txt(" Scroll"));
                hints
            }
        };

        let mut lines = vec![Line::from(hints)];
        // Transient status flash below the hints.
        if let Some((at, msg)) = &self.status_line {
            if at.elapsed().as_secs() < 4 {
                lines.push(Line::from(Span::styled(
                    msg.clone(),
                    Style::default().fg(CATPPUCCIN.teal).add_modifier(Modifier::ITALIC),
                )));
            }
        }

        let footer = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(CATPPUCCIN.subtext0));
        f.render_widget(footer, area);
    }

    /// Bordered info table with label/value columns and striped rows.
    fn info_table<'b>(title: &'static str, rows: Vec<(&'static str, Span<'b>)>) -> Table<'b> {
        let rows: Vec<Row> = rows
            .into_iter()
            .enumerate()
            .map(|(i, (label, value))| {
                let bg = if i % 2 == 0 { APP_BACKGROUND } else { CATPPUCCIN.base };
                Row::new(vec![
                    Cell::from(Span::styled(label, Style::default().fg(THEME.tertiary))),
                    Cell::from(value),
                ])
                .style(Style::default().bg(bg))
            })
            .collect();

        Table::new(rows, [Constraint::Length(14), Constraint::Fill(1)])
            .column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(THEME.border(false))
                    .title(title)
                    .title_style(THEME.title()),
            )
    }

    pub(crate) fn draw_ticket_page(&self, f: &mut Frame, area: Rect) {
        let task = &self.task;
        let ticket = self.ticket.borrow();
        let customer = self.customer.borrow();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // Info tables
                Constraint::Min(5),     // Description
            ])
            .split(area);

        let info_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[0]);

        // Task / ticket info table
        let text_value = |s: String| Span::styled(s, Style::default().fg(CATPPUCCIN.text));
        let mut ticket_rows: Vec<(&'static str, Span)> = vec![
            ("Service #", text_value(task.service_number.clone().unwrap_or_else(|| "—".into()))),
            ("Status", Span::styled(task.status.as_str().to_string(), Self::status_style(&task.status))),
            ("Priority", Span::styled(task.priority.as_str().to_string(), Self::priority_style(&task.priority))),
            ("Assignee", text_value(self.username_for(&task.assignee))),
            ("Due Date", text_value(task.due_date.format("%m/%d/%Y").to_string())),
            (
                "Completed",
                Span::styled(
                    if task.completed { "Yes" } else { "No" },
                    Style::default().fg(if task.completed { THEME.success } else { THEME.warning }),
                ),
            ),
        ];
        if let Some(ref ticket) = *ticket {
            ticket_rows.push(("Tech", text_value(ticket.tech.clone())));
            ticket_rows.push(("Salesman", text_value(ticket.salesman.clone())));
            if !ticket.doc_alias.is_empty() {
                ticket_rows.push(("Order Type", text_value(ticket.doc_alias.clone())));
            }
            if !ticket.ticket_total.is_empty() {
                ticket_rows.push(("Total", text_value(format!("${}", ticket.ticket_total))));
            }
        }
        f.render_widget(Self::info_table(" Ticket ", ticket_rows), info_layout[0]);

        // Customer info table
        let customer_rows: Vec<(&'static str, Span)> = if let Some(ref cust) = *customer {
            vec![
                ("Customer", text_value(cust.name.clone())),
                ("Phone", text_value(cust.phone_number.clone())),
                ("Email", text_value(cust.email.clone())),
                ("Cust Code", text_value(cust.cust_code.clone())),
            ]
        } else {
            vec![("Customer", Span::styled("Loading…", Style::default().fg(CATPPUCCIN.subtext0)))]
        };
        f.render_widget(Self::info_table(" Customer ", customer_rows), info_layout[1]);

        // Description section: editable with 'e'.
        let editing = *self.focus.borrow() == ModalFocus::EditDescription;
        let desc_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(editing))
            .title(if editing { " Description (Ctrl+S save, Esc cancel) " } else { " Description (e to edit) " })
            .title_style(THEME.title());

        if editing {
            let inner = desc_block.inner(layout[1]);
            f.render_widget(desc_block, layout[1]);
            f.render_widget(&self.description_editor, inner);
        } else {
            let desc_para = Paragraph::new(task.task_description.clone())
                .block(desc_block)
                .wrap(Wrap { trim: true })
                .scroll((*self.scroll_offset.borrow(), 0))
                .style(Style::default().fg(CATPPUCCIN.text));
            f.render_widget(desc_para, layout[1]);
        }
    }

    pub(crate) fn draw_computer_page(&self, f: &mut Frame, area: Rect) {
        let computer = self.computer.borrow();

        let text_value = |s: String| Span::styled(s, Style::default().fg(CATPPUCCIN.text));
        let opt_value = |s: Option<&str>| {
            match s {
                Some(v) if !v.is_empty() => Span::styled(v.to_string(), Style::default().fg(CATPPUCCIN.text)),
                _ => Span::styled("N/A", Style::default().fg(CATPPUCCIN.subtext0)),
            }
        };

        let rows: Vec<(&'static str, Span)> = if let Some(ref comp) = *computer {
            vec![
                ("Hostname", text_value(comp.hostname.clone())),
                ("CPU", text_value(comp.cpu.clone())),
                ("GPU", text_value(comp.gpu.clone())),
                ("RAM", text_value(comp.ram.clone())),
                ("OS", text_value(comp.operating_system.clone())),
                ("Drives", text_value(format!("{} detected", comp.drives.len()))),
                (
                    "Win Active",
                    match comp.windows_active {
                        Some(true) => Span::styled("Yes", Style::default().fg(THEME.success)),
                        Some(false) => Span::styled("No", Style::default().fg(THEME.error)),
                        None => Span::styled("Unknown", Style::default().fg(CATPPUCCIN.subtext0)),
                    },
                ),
                ("Device Name", opt_value(comp.device_name.as_deref())),
                ("Device Mfg", opt_value(comp.device_mfg.as_deref())),
                ("Device Model", opt_value(comp.device_model.as_deref())),
                ("Device Serial", opt_value(comp.device_serial.as_deref())),
                ("Product", text_value(comp.product_name.clone())),
                ("Vendor", text_value(comp.product_vendor.clone())),
                ("Motherboard", text_value(comp.motherboard_name.clone())),
                ("MB Vendor", text_value(comp.motherboard_vendor.clone())),
                ("MB Serial", text_value(comp.motherboard_serial.clone())),
            ]
        } else {
            vec![("Computer", Span::styled("Loading…", Style::default().fg(CATPPUCCIN.subtext0)))]
        };

        f.render_widget(Self::info_table(" Computer ", rows), area);
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
                    for program in arr.iter() {
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
            .block(Block::default().title(" Software ").title_style(THEME.title()))
            .scroll((*self.scroll_offset.borrow(), 0));
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
            .block(Block::default().title(" Task History ").title_style(THEME.title()))
            .scroll((*self.scroll_offset.borrow(), 0));
        f.render_widget(para, area);
    }

    /// Chat-style notes page: message bubbles plus a compose box.
    pub(crate) fn draw_notes_page(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),    // Messages
                Constraint::Length(5), // Compose box
            ])
            .split(area);

        self.draw_chat_messages(f, layout[0]);
        self.draw_note_input(f, layout[1]);
    }

    fn draw_chat_messages(&self, f: &mut Frame, area: Rect) {
        let notes = self.notes.borrow();
        let my_username = self.current_user.get_username().to_string();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(false))
            .title(format!(" Task Notes ({}) ", notes.len()))
            .title_style(THEME.title());
        let inner = block.inner(area);
        f.render_widget(block, area);

        if notes.is_empty() {
            let empty = Paragraph::new("No notes yet — press i to write one")
                .style(Style::default().fg(CATPPUCCIN.subtext0))
                .alignment(Alignment::Center);
            f.render_widget(empty, inner);
            return;
        }

        let bubble_width = ((inner.width as f32) * 0.72).max(20.0) as usize;
        let mut lines: Vec<Line> = Vec::new();

        for note in notes.iter() {
            let mine = note.username == my_username;
            let alignment = if mine { Alignment::Right } else { Alignment::Left };
            let name_color = if mine { THEME.accent } else { CATPPUCCIN.blue };
            let bubble_bg = if mine { CATPPUCCIN.surface1 } else { CATPPUCCIN.surface0 };

            lines.push(Self::chat_header_line(note, name_color, alignment));
            for text_line in Self::wrap_text(&note.note, bubble_width.saturating_sub(2)) {
                lines.push(
                    Line::from(Span::styled(
                        format!(" {text_line} "),
                        Style::default().fg(CATPPUCCIN.text).bg(bubble_bg),
                    ))
                    .alignment(alignment),
                );
            }
            lines.push(Line::raw(""));
        }

        // Scroll anchored to the bottom: notes_scroll counts lines back up.
        let total = lines.len() as u16;
        let visible = inner.height;
        let max_scroll = total.saturating_sub(visible);
        let from_bottom = (*self.notes_scroll.borrow()).min(max_scroll);
        if *self.notes_scroll.borrow() > max_scroll {
            *self.notes_scroll.borrow_mut() = max_scroll;
        }
        let offset_top = max_scroll - from_bottom;

        let para = Paragraph::new(lines).scroll((offset_top, 0));
        f.render_widget(para, inner);
    }

    /// Header line for one chat message: name, timestamp and tags.
    fn chat_header_line(note: &TaskNotePayload, name_color: Color, alignment: Alignment) -> Line<'static> {
        let created: chrono::DateTime<chrono::Utc> = note.created_at.clone().into();
        let created = created.with_timezone(&chrono::Local);
        let mut spans = vec![
            Span::styled(
                note.username.clone(),
                Style::default().fg(name_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", created.format("%m/%d/%y %H:%M")),
                Style::default().fg(CATPPUCCIN.subtext0),
            ),
        ];
        if note.private {
            spans.push(Span::styled(
                "  private",
                Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::ITALIC),
            ));
        }
        if note.id_customer_message.is_some() {
            spans.push(Span::styled(
                "  PS",
                Style::default().fg(CATPPUCCIN.sky).add_modifier(Modifier::ITALIC),
            ));
        }
        Line::from(spans).alignment(alignment)
    }

    fn draw_note_input(&self, f: &mut Frame, area: Rect) {
        *self.note_input_area.borrow_mut() = area;
        let focused = *self.focus.borrow() == ModalFocus::NoteInput;

        let title = if self.sending_note {
            " Sending… ".to_string()
        } else if self.note_private {
            " New Note 🔒 PRIVATE ".to_string()
        } else {
            " New Note ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(focused))
            .title(title)
            .title_style(if self.note_private {
                Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)
            } else {
                THEME.title()
            });
        let inner = block.inner(area);
        f.render_widget(block, area);
        f.render_widget(&self.note_editor, inner);
    }

    /// Selector popup for status / assignee / priority.
    fn draw_selector_popup(&self, f: &mut Frame, modal_area: Rect) {
        if *self.focus.borrow() != ModalFocus::Selector {
            return;
        }
        let Some(selector) = self.selector.as_ref() else { return; };
        let labels = selector.labels();
        let selected = selector.idx();

        let width = labels
            .iter()
            .map(|l| l.width())
            .max()
            .unwrap_or(10)
            .max(selector.title().width()) as u16
            + 8;
        let height = labels.len() as u16 + 2;
        let x = modal_area.x + (modal_area.width.saturating_sub(width)) / 2;
        let y = modal_area.y + (modal_area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width.min(modal_area.width), height.min(modal_area.height));

        f.render_widget(Clear, popup_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border(true))
            .style(Style::default().bg(APP_BACKGROUND))
            .title(selector.title())
            .title_style(THEME.title());
        let inner = block.inner(popup_area);
        f.render_widget(block, popup_area);

        for (i, label) in labels.iter().enumerate() {
            let y = inner.y + i as u16;
            if y >= inner.bottom() {
                break;
            }
            let (marker, style) = if i == selected {
                (" ▶ ", THEME.menu_highlight())
            } else {
                ("   ", Style::default().fg(CATPPUCCIN.text))
            };
            let line = Paragraph::new(format!("{marker}{label}")).style(style);
            f.render_widget(line, Rect { x: inner.x, y, width: inner.width, height: 1 });
        }
    }

    /// Word-wrap text to a maximum display width.
    fn wrap_text(text: &str, width: usize) -> Vec<String> {
        let width = width.max(8);
        let mut out = Vec::new();
        for raw_line in text.lines() {
            if raw_line.width() <= width {
                out.push(raw_line.to_string());
                continue;
            }
            let mut current = String::new();
            for word in raw_line.split_whitespace() {
                if current.is_empty() {
                    current = word.to_string();
                } else if current.width() + 1 + word.width() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    out.push(std::mem::take(&mut current));
                    current = word.to_string();
                }
                // Hard-break words longer than the bubble width.
                while current.width() > width {
                    let split_at = current
                        .char_indices()
                        .scan(0usize, |acc, (i, c)| {
                            *acc += c.to_string().width();
                            Some((i, *acc))
                        })
                        .find(|(_, w)| *w > width)
                        .map(|(i, _)| i)
                        .unwrap_or(current.len());
                    let rest = current.split_off(split_at.max(1));
                    out.push(std::mem::take(&mut current));
                    current = rest;
                }
            }
            if !current.is_empty() {
                out.push(current);
            }
        }
        if out.is_empty() {
            out.push(String::new());
        }
        out
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

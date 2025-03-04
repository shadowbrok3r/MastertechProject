use ratatui::{crossterm::event::{KeyCode, KeyModifiers, MouseEvent}, layout::{Constraint, Direction, Layout, Rect, Size}, prelude::{Backend, StatefulWidget}, style::{Style, Stylize}, symbols::border::Set, text::{Line, Span}, widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Widget, WidgetRef, Wrap}, Frame};
use crate::terminal_mode::{styling::{BASE_COLORS, CATPPUCCIN, CYAN, DARKORANGE}, tabs::checklist::TodoItem, widgets::{ButtonType, HandleWidget, ShrinkArea, _SHORTCUT_SET_2}};
use super::{checklist::Status, ScriptsTab, ScriptsTabView};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

#[derive(Clone, Debug)]
pub struct Report {
    pub reporter: Reporter,
    pub msg: String,
}

#[derive(Clone, Debug)]
pub enum Reporter {
    Tuneup,
    Qc,
    WindowsUpdates,
    GetAntivirus,
    GetInstalledPrograms,
    GetStartupItems,
    GetScheduledTasks,
    GetTaskbarItems,
    RunPrechecks,
    Unknown
}

/// Helper enum for movement direction
#[derive(Clone, Copy, Debug)]
pub enum MoveDirection {
    Up,
    Down,
}

impl<'a> ScriptsTab<'a> {
    fn draw_antivirus<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_antivirus_btn, layout[0]);

        let antivirus_data: Vec<Line> = self.antivirus_products.iter().map(|av| {
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(CYAN.text)),
                Span::raw(&av.display_name),
                Span::styled(" | State: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(av.product_state.to_string()),
            ])
        }).collect();

        let paragraph = Paragraph::new(antivirus_data)
            .block(Block::default().borders(Borders::ALL).title("Antivirus Info").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_installed_programs<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_installed_programs_btn, layout[0]);

        let programs_data: Vec<Line> = self.installed_programs.iter().map(|prog| {
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(CYAN.text)),
                Span::raw(prog.display_name.clone().unwrap_or_default()),
                Span::styled(" | Version: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(prog.display_version.clone().unwrap_or_default()),
            ])
        }).collect();

        let paragraph = Paragraph::new(programs_data)
            .block(Block::default().borders(Borders::ALL).title("Installed Programs").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_startup_items<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_startup_items_btn, layout[0]);

        let startup_data: Vec<Line> = self.startup_programs.iter().map(|item| {
            Line::from(vec![
                Span::styled("Key: ", Style::default().fg(CYAN.text)),
                Span::raw(&item.key_name),
                Span::styled(" | State: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(format!("{:?}", item.decoded_state)),
            ])
        }).collect();

        let paragraph = Paragraph::new(startup_data)
            .block(Block::default().borders(Borders::ALL).title("Startup Items").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_scheduled_tasks<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_scheduled_tasks_btn, layout[0]);

        let task_data: Vec<Line> = self.scheduled_tasks.iter().map(|task| {
            Line::from(vec![
                Span::styled("Task Name: ", Style::default().fg(CYAN.text)),
                Span::raw(task.task_name.clone().unwrap_or_default()),
                Span::styled(" | State: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(format!("{:?}", task.state)),
            ])
        }).collect();

        let paragraph = Paragraph::new(task_data)
            .block(Block::default().borders(Borders::ALL).title("Scheduled Tasks").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_taskbar_items<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        f.render_widget(&self.get_taskbar_items_btn, layout[0]);

        let taskbar_data: Vec<Line> = self.taskbar_items.iter().map(|item| {
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(CYAN.text)),
                Span::raw(&item.name),
                Span::styled(" | Path: ", Style::default().fg(DARKORANGE.text)),
                Span::raw(&item.path),
            ])
        }).collect();

        let paragraph = Paragraph::new(taskbar_data)
            .block(Block::default().borders(Borders::ALL).title("Taskbar Items").border_type(BorderType::Rounded))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, layout[1]);
    }

    fn draw_log_section<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35),  // Checklist 
                Constraint::Percentage(65),  // Log Messages
            ])
            .split(area);

        // Render checklists
        let mut checklist_items: Vec<ListItem> = Vec::new();
        let mut item_to_flat_index: Vec<usize> = Vec::new(); // Maps item index to flattened TodoItem index
        let mut flat_index = 0;

        for list in self.checklists.values() {
            // Add header (not selectable)
            checklist_items.push(ListItem::new(Line::styled(
                format!("📌 {}", list.name),
                Style::default().fg(CATPPUCCIN.sapphire).bold(),
            )));

            // Add items (selectable)
            for item in &list.items {
                let symbol = match item.status {
                    Status::Completed => "✓",
                    Status::Todo => "☐",
                };
                let style = match item.status {
                    Status::Completed => Style::default().fg(CATPPUCCIN.teal),
                    Status::Todo => Style::default().fg(CATPPUCCIN.pink),
                };
                checklist_items.push(ListItem::new(Line::styled(
                    format!("{} {}", symbol, item.text),
                    style,
                )));
                item_to_flat_index.push(flat_index);
                flat_index += 1;
            }
        }
                    // Adjust selection to skip headers
        let mut list_state = self.list_state.borrow_mut();
        if let Some(selected) = list_state.selected() {
            if selected < checklist_items.len() && !item_to_flat_index.contains(&selected) {
                // Selected a header; move to the next valid item
                let next_valid = (selected + 1..checklist_items.len())
                    .find(|&i| item_to_flat_index.contains(&i))
                    .or_else(|| (0..selected).rev().find(|&i| item_to_flat_index.contains(&i)));
                list_state.select(next_valid);
            }
        }
        
        // Calculate virtual dimensions for the checklist ScrollView
        // let list_visible_height = layout[0].height.saturating_sub(2); // Subtract borders
        let list_virtual_height = checklist_items.len() as u16; // Total number of items (including headers)
        let max_line_width = checklist_items
            .iter()
            .map(|item| item.width() as u16)
            .max()
            .unwrap_or(layout[0].width);
        let list_visible_width = layout[0].width.saturating_sub(2); // Subtract borders
        let list_virtual_width = max_line_width.max(list_visible_width);

        let list_virtual_size = Size {
            width: list_virtual_width,
            height: list_virtual_height,
        };

        // Create checklist ScrollView
        let mut list_scroll_view = ScrollView::new(list_virtual_size)
            .vertical_scrollbar_visibility(ScrollbarVisibility::Automatic)
            .horizontal_scrollbar_visibility(ScrollbarVisibility::Automatic);

        let checklist = List::new(checklist_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Job Builder")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(CATPPUCCIN.lavender))
            )
            .highlight_symbol("=> ")
            .highlight_style(Style::new().bg(CATPPUCCIN.base).fg(CATPPUCCIN.teal));
    
        // f.render_stateful_widget(checklist, layout[0], &mut list_state);
        self.checklist_area.replace(Some(layout[0]));

        // Render the List into the checklist ScrollView
        list_scroll_view.render_widget(checklist, list_scroll_view.area());

        // Render the checklist ScrollView into the frame
        list_scroll_view.render(layout[0], f.buffer_mut(), &mut self.list_scroll_state.borrow_mut());

        // 📝 Render logs
        let log_text: Vec<Line> = self.reports.borrow().iter().enumerate().map(|(index, r)| {
            let color = BASE_COLORS[index % BASE_COLORS.len()];
        
            // Extract message and split at " => "
            let (left_text, right_text) = match r.msg.split_once(" => ") {
                Some((left, right)) => (left.trim(), right.trim()), // Trim spaces
                None => (r.msg.as_str(), ""), // If no " => ", treat it as left_text
            };
        
            // Get the available width
            let available_width = layout[1].width as usize;
        
            // Get the length of the reporter field to subtract from available width
            let reporter_text = format!("{:?} =>", r.reporter);
            let reporter_length = reporter_text.len();
        
            // Calculate remaining width for right-aligned text
            let width = available_width.saturating_sub(left_text.len() + reporter_length ); // +2 for extra spacing
        
            // Ensure formatted message stays within available width
            let formatted_msg = format!("{:<} {:>width$}", left_text, right_text, width = width);
        
            Line::from(vec![
                Span::styled(format!("{} {}", reporter_text, formatted_msg), Style::default().fg(color))
            ])
        }).collect();
        
        // Calculate virtual dimensions
        let log_lines = log_text.len() as u16; // Number of lines for vertical scrolling
        let visible_height = layout[1].height.saturating_sub(2); // Subtract borders
        let virtual_height = log_lines.max(visible_height); // Dynamic height based on content

        // Calculate the maximum line width for horizontal scrolling
        let max_line_width = log_text.iter()
            .map(|line| line.width() as u16) // Width of each line in characters
            .max()
            .unwrap_or(layout[1].width); // Default to visible width if no lines
        let visible_width = layout[1].width.saturating_sub(2); // Subtract borders
        let virtual_width = max_line_width.max(visible_width); // Dynamic width based on longest line

        let virtual_size = Size {
            width: virtual_width,
            height: virtual_height,
        };   

        let mut scroll_view = ScrollView::new(virtual_size)
            .vertical_scrollbar_visibility(
                ScrollbarVisibility::Automatic
            )
            .horizontal_scrollbar_visibility(
                ScrollbarVisibility::Automatic
            );
            
        self.report_area.replace(Some(layout[1]));

        let log_widget = Paragraph::new(log_text)
            .block(Block::default().borders(Borders::ALL).style(Style::new().fg(CATPPUCCIN.blue)).title("Run Report").border_type(BorderType::Rounded));
            // .wrap(Wrap { trim: false });
    
        scroll_view.render_widget(log_widget, scroll_view.area());

        scroll_view.render(layout[1], f.buffer_mut(), &mut self.report_scroll_state.borrow_mut());
    } 

    /// Move an item up or down, potentially across categories
    fn move_item(&mut self, direction: MoveDirection, selected: usize) {
        let (full_list, item_to_flat_index) = self.build_full_list();
        if full_list.is_empty() || !item_to_flat_index.contains(&selected) {
            return; // No items or selected a header
        }

        let flat_selected = item_to_flat_index.iter().position(|&i| i == selected).unwrap();
        let (current_category, current_item) = full_list[flat_selected].clone();

        match direction {
            MoveDirection::Up if flat_selected > 0 => {
                let local_index = self.checklists[&current_category]
                    .items
                    .iter()
                    .position(|item| *item == current_item)
                    .unwrap();
                if local_index == 0 {
                    let prev_category_idx = full_list[..flat_selected]
                        .iter()
                        .rposition(|(name, _)| *name != current_category)
                        .unwrap();
                    let (prev_category_name, _) = &full_list[prev_category_idx];
                    let item = self.checklists.get_mut(&current_category).unwrap().items.remove(0);
                    self.checklists.get_mut(prev_category_name).unwrap().items.push(item);
                    self.list_state.borrow_mut().select(Some(item_to_flat_index[flat_selected - 1]));
                } else {
                    self.checklists
                        .get_mut(&current_category)
                        .unwrap()
                        .items
                        .swap(local_index, local_index - 1);
                    self.list_state.borrow_mut().select(Some(item_to_flat_index[flat_selected - 1]));
                }
            }
            MoveDirection::Down if flat_selected < full_list.len() - 1 => {
                let local_index = self.checklists[&current_category]
                    .items
                    .iter()
                    .position(|item| *item == current_item)
                    .unwrap();
                if local_index == self.checklists[&current_category].items.len() - 1 {
                    let next_category_idx = full_list[flat_selected + 1..]
                        .iter()
                        .position(|(name, _)| *name != current_category)
                        .map(|i| i + flat_selected + 1)
                        .unwrap_or(full_list.len() - 1);
                    let (next_category_name, _) = &full_list[next_category_idx];
                    let item = self.checklists.get_mut(&current_category).unwrap().items.remove(local_index);
                    self.checklists.get_mut(next_category_name).unwrap().items.insert(0, item);
                    self.list_state.borrow_mut().select(Some(item_to_flat_index[flat_selected + 1]));
                } else {
                    self.checklists
                        .get_mut(&current_category)
                        .unwrap()
                        .items
                        .swap(local_index, local_index + 1);
                    self.list_state.borrow_mut().select(Some(item_to_flat_index[flat_selected + 1]));
                }
            }
            _ => {}
        }
    }

    /// Build the flattened list and index mapping
    fn build_full_list(&self) -> (Vec<(String, TodoItem)>, Vec<usize>) {
        let mut full_list: Vec<(String, TodoItem)> = Vec::new();
        let mut item_to_flat_index: Vec<usize> = Vec::new();
        let mut checklist_items_count = 0;

        for (name, list) in self.checklists.iter() {
            checklist_items_count += 1; // Header
            for item in &list.items {
                full_list.push((name.clone(), item.clone()));
                item_to_flat_index.push(checklist_items_count);
                checklist_items_count += 1;
            }
        }
        (full_list, item_to_flat_index)
    }

    /// Select the first valid (non-header) item
    fn select_first_valid(&self, list_state: &mut ListState) {
        let (_, item_to_flat_index) = self.build_full_list();
        if let Some(&first) = item_to_flat_index.first() {
            list_state.select(Some(first));
        }
    }

    /// Select the previous valid (non-header) item
    fn select_previous_valid(&self, list_state: &mut ListState) {
        let (_, item_to_flat_index) = self.build_full_list();
        if let Some(selected) = list_state.selected() {
            if let Some(prev) = item_to_flat_index
                .iter()
                .rposition(|&i| i < selected)
                .map(|i| item_to_flat_index[i])
            {
                list_state.select(Some(prev));
            }
        }
    }

    /// Select the next valid (non-header) item
    fn select_next_valid(&self, list_state: &mut ListState) {
        let (_, item_to_flat_index) = self.build_full_list();
        if let Some(selected) = list_state.selected() {
            if let Some(next) = item_to_flat_index
                .iter()
                .position(|&i| i > selected)
                .map(|i| item_to_flat_index[i])
            {
                list_state.select(Some(next));
            }
        } else {
            self.select_first_valid(list_state);
        }
    } 
}


impl<'a> HandleWidget<'_> for ScriptsTab<'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.receive();
        self.update_selected_tab();
        
        // Define layout sections
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab row
                Constraint::Percentage(98), // Content
            ])
            .split(area);
    
        let tab_row = main_layout[0]; // Tab buttons
        let content_area = main_layout[1]; // Dynamic content based on selected tab

        // Layout for tab buttons
        let tab_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, self.tab_buttons.len() as u32); self.tab_buttons.len()])
            .split(tab_row);
    
        // Render tab buttons in order
        for (i, (_, button)) in self.tab_buttons.iter().enumerate() {
            f.render_widget(button, tab_layout[i]);
        }
    

        // Display content based on selected tab
        match *self.current_tab.borrow() {
            ScriptsTabView::Main => {
                // Split area into buttons (left) and logs (right)
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(20), // Left: Buttons
                        Constraint::Percentage(80), // Right: Logs
                    ])
                    .split(content_area);
    
                let left_half = main_chunks[0];
                let right_half = main_chunks[1];

                let left_side_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(10),
                    Constraint::Percentage(90),
                ])
                .split(left_half);
                // let top = left_half
                let para = Paragraph::new("Scripts Library")
                    .block(
                        Block::default().bg(CATPPUCCIN.base)
                    )
                    .centered();

                para.render_ref(left_side_chunks[0], f.buffer_mut());

                // Create grid layout for buttons
                let button_grid = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(vec![Constraint::Ratio(1, 8); 8])
                    .split(left_side_chunks[1]);
                
                // Render main buttons
                f.render_widget(&self.tuneup_btn, button_grid[0].shrink(5, 1));
                f.render_widget(&self.qc_btn, button_grid[1].shrink(5, 1));
                f.render_widget(&self.updates_btn, button_grid[2].shrink(5, 1));
                f.render_widget(&self.prechecks_btn, button_grid[3].shrink(5, 1));
                // Render log section
                self.draw_log_section::<B>(f, right_half);
            }
            ScriptsTabView::Antivirus => self.draw_antivirus::<B>(f, content_area),
            ScriptsTabView::StartupItems => self.draw_startup_items::<B>(f, content_area),
            ScriptsTabView::InstalledPrograms => self.draw_installed_programs::<B>(f, content_area),
            ScriptsTabView::ScheduledTasks => self.draw_scheduled_tasks::<B>(f, content_area),
            ScriptsTabView::TaskbarItems => self.draw_taskbar_items::<B>(f, content_area),
        }

        
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let c = mouse_event.column;
        let r = mouse_event.row;
        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                if let Some(report_area) = *self.report_area.borrow() {
                    if c >= report_area.x && c < report_area.x + report_area.width &&
                        r >= report_area.y && r < report_area.y + report_area.height 
                    {
                        self.report_scroll_state.borrow_mut().scroll_down();
                        self.report_scroll_state.borrow_mut().scroll_down();
                    }
                };
                if let Some(list_area) = *self.checklist_area.borrow() {
                    if c >= list_area.x && c < list_area.x + list_area.width &&
                        r >= list_area.y && r < list_area.y + list_area.height 
                    {
                        self.list_scroll_state.borrow_mut().scroll_down();
                        self.list_scroll_state.borrow_mut().scroll_down();
                    }
                };
            },
            ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                if let Some(report_area) = *self.report_area.borrow() {
                    if c >= report_area.x && c < report_area.x + report_area.width &&
                        r >= report_area.y && r < report_area.y + report_area.height 
                    {
                        self.report_scroll_state.borrow_mut().scroll_up();
                        self.report_scroll_state.borrow_mut().scroll_up();
                    }
                };
                if let Some(list_area) = *self.checklist_area.borrow() {
                    if c >= list_area.x && c < list_area.x + list_area.width &&
                        r >= list_area.y && r < list_area.y + list_area.height 
                    {
                        self.list_scroll_state.borrow_mut().scroll_up();
                        self.list_scroll_state.borrow_mut().scroll_up();
                    }
                };
            },
            ratatui::crossterm::event::MouseEventKind::ScrollLeft => self.report_scroll_state.borrow_mut().scroll_left(),
            ratatui::crossterm::event::MouseEventKind::ScrollRight => self.report_scroll_state.borrow_mut().scroll_right(),
            _ => {
                self.tuneup_btn.handle_mouse_event(&mouse_event);
                self.qc_btn.handle_mouse_event(&mouse_event);
                self.prechecks_btn.handle_mouse_event(&mouse_event);
                self.updates_btn.handle_mouse_event(&mouse_event);
                self.get_antivirus_btn.handle_mouse_event(&mouse_event);
                self.get_installed_programs_btn.handle_mouse_event(&mouse_event);
                self.get_startup_items_btn.handle_mouse_event(&mouse_event);
                self.get_scheduled_tasks_btn.handle_mouse_event(&mouse_event);
                self.get_taskbar_items_btn.handle_mouse_event(&mouse_event);
                for (_id, button) in self.tab_buttons.iter() {
                    button.handle_mouse_event(&mouse_event);
                }
            }
        }
    }

    fn handle_key_event(&mut self, key_event: ratatui::crossterm::event::KeyEvent) -> bool {
        log::info!("KEY EVENT: {key_event:?}");
        match key_event.code {
            KeyCode::Right => {
                log::info!("RIGHT");
                self.report_scroll_state.borrow_mut().scroll_right();
                self.report_scroll_state.borrow_mut().scroll_right();
                true
            },
            KeyCode::Left => {
                self.report_scroll_state.borrow_mut().scroll_left();
                self.report_scroll_state.borrow_mut().scroll_left();
                true
            },
            KeyCode::Up => {
                log::info!("UP");
                let mut list_state = self.list_state.borrow_mut();
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    let selected = list_state.selected();
                    drop(list_state); // Drop the borrow before calling move_item
                    if let Some(selected) = selected {
                        self.move_item(MoveDirection::Up, selected);
                    }
                } else {
                    if list_state.selected().is_none() {
                        self.select_first_valid(&mut list_state);
                    } else {
                        self.select_previous_valid(&mut list_state);
                    }
                }
                true
            }
            KeyCode::Down => {
                log::info!("DOWN");
                let mut list_state = self.list_state.borrow_mut();
                let selected = list_state.selected().clone();
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    self.log_message(&format!("CTRL DOWN: {key_event:?}"));
                    let selected = list_state.selected();
                    drop(list_state); // Drop the borrow before calling move_item
                    if let Some(selected) = selected {
                        self.move_item(MoveDirection::Down, selected);
                    }
                } else {
                    if list_state.selected().is_none() {
                        self.select_first_valid(&mut list_state);
                    } else {
                        self.select_next_valid(&mut list_state);
                    }
                }
                log::info!("Selected: {:?}", selected);
                true
            }
            KeyCode::Esc => {
                let mut list_state = self.list_state.borrow_mut();
                list_state.select(None);
                true
            }
            KeyCode::Enter => {
                let list_state = self.list_state.borrow();
                if let Some(selected) = list_state.selected() {
                    let (full_list, item_to_flat_index) = self.build_full_list();

                    let flat_selected = item_to_flat_index.iter().position(|&i| i == selected).unwrap();
                    let (current_category, current_item) = full_list[flat_selected].clone();
                    log::info!("Current Category: {:?}\nCurrent Item: {:?}", current_category, current_item);
                }
                true
            }
            _ => false
        }
    }
}
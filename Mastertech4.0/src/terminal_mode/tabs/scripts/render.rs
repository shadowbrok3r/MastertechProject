use ratatui::{crossterm::event::{KeyCode, KeyModifiers, MouseEvent}, layout::{Constraint, Direction, Layout, Rect, Size}, prelude::{Backend, StatefulWidget}, style::{Color, Style, Stylize}, text::{Line, Span}, widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap}, Frame};
use tui_scrollview::{ScrollView, ScrollbarVisibility};
use crate::terminal_mode::{styling::{BASE_COLORS, CATPPUCCIN, CYAN, DARKORANGE}, tabs::checklist::TodoItem, widgets::{button::ButtonState, ButtonType, HandleWidget, ShrinkArea}};
use super::{checklist::Status, ScriptsTab, ScriptsTabView};
// use tui_popup::Popup;

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

        // 📝 Render checklists
        let checklist_items: Vec<ListItem> = self.checklists.values().flat_map(|list| {
            let header = ListItem::new(Line::styled(format!("📌 {}", list.name), Style::default().fg(CATPPUCCIN.sapphire).bold()));
            let tasks = list.items.iter().map(|item| {
                let symbol = match item.status {
                    Status::Completed => "✓",
                    Status::Todo => "☐",
                };
                let style = match item.status {
                    Status::Completed => Style::default().fg(CATPPUCCIN.teal),
                    Status::Todo => Style::default().fg(CATPPUCCIN.pink),
                };
                ListItem::new(Line::styled(format!("{} {}", symbol, item.text), style))
            });
    
            std::iter::once(header).chain(tasks).collect::<Vec<_>>()
        }).collect();
    
        let checklist = List::new(checklist_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Checklist")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(CATPPUCCIN.lavender))
            )
            .highlight_symbol("=> ")
            .highlight_style(Style::new().bg(CATPPUCCIN.base).fg(CATPPUCCIN.teal));
    
        f.render_stateful_widget(checklist, layout[0], &mut self.list_state.borrow_mut());
    
        // // Calculate virtual dimensions
        // let checklist_item_lines = checklist_items.len() as u16; // Number of lines for vertical scrolling
        // let visible_height = layout[0].height.saturating_sub(2); // Subtract borders
        // let virtual_height = checklist_item_lines.max(visible_height); // Dynamic height based on content
        // let checklist_virtual_size = Size {
        //     width: layout[0].width,
        //     height: virtual_height,
        // };   
        // let mut checklist_scroll_view = ScrollView::new(checklist_virtual_size)
        //     .vertical_scrollbar_visibility(
        //         ScrollbarVisibility::Automatic
        //     )
        //     .horizontal_scrollbar_visibility(
        //         ScrollbarVisibility::Automatic
        //     );
        // let checklist_rect = scroll_view.area();
        // checklist_scroll_view.render_widget(checklist, checklist_rect);
        // checklist_scroll_view.render(layout[0], f.buffer_mut(), &mut self.checklist_scroll_state.borrow_mut());

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
            
        let rect = scroll_view.area();

        let log_widget = Paragraph::new(log_text)
            .block(Block::default().borders(Borders::ALL).style(Style::new().fg(CATPPUCCIN.blue)).title("Run Report").border_type(BorderType::Rounded));
            // .wrap(Wrap { trim: false });
    
        scroll_view.render_widget(log_widget, rect);

        scroll_view.render(layout[1], f.buffer_mut(), &mut self.report_scroll_state.borrow_mut());
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

                // Create grid layout for buttons
                let button_grid = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(vec![Constraint::Ratio(1, 8); 8])
                    .split(left_half);
    
                // let button_row1 = Layout::default()
                //     .direction(Direction::Horizontal)
                //     .constraints([Constraint::Percentage(60)])
                //     .split(button_grid[0]);
    
                // let button_row2 = Layout::default()
                //     .direction(Direction::Horizontal)
                //     .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                //     .split(button_grid[1]);
    
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
        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                self.report_scroll_state.borrow_mut().scroll_down();
                self.report_scroll_state.borrow_mut().scroll_down();
            },
            ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                self.report_scroll_state.borrow_mut().scroll_up();
                self.report_scroll_state.borrow_mut().scroll_up();
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
                if list_state.selected().is_none() {
                    list_state.select_first();
                } else {
                    list_state.select_previous();
                }
                true
            },
            KeyCode::Down => {
                log::info!("DOWN");
                let mut list_state = self.list_state.borrow_mut();
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    self.log_message("CTRL DOWN: {key_event:?}");
                    let selected = list_state.selected();
                    if let Some(i) = selected {
                        let full_list = self
                            .checklists
                            .iter()
                            .flat_map(|(_, list)| 
                                list.items.iter().cloned()
                            )
                            .collect::<Vec<TodoItem>>();

                        for (name, list) in self.checklists.iter_mut() {
                            let todo_item = full_list.get(i).is_some();
                            let next_item = full_list.get(i+1).is_some();
                            let previous_item = full_list.get(i-1).is_some();
                            for (x, y) in list.items.iter().zip(&full_list) {
                                if x == y {
                                    /*
                                        Now, we have to get the index of x and y within their categories, and their
                                        category names
                                     */
                                    log::info!("Found {x:?} in list items, {y:?} in full_list: {:?} // {:?}", i, name);
                                }
                            }
                            log::info!("{full_list:?}\ntodo_item: {todo_item:?}, next_item: {next_item:?}, previous_item: {previous_item:?}");
                            if todo_item && next_item {
                                list.items.swap(i, i+1);
                            } else if todo_item && previous_item {
                                list.items.swap(i, i-1);
                            }
                        }




                        
                        // if let Some(todo_item) =  {
                        //     if let Some(next_item) =  {
                        //         // full_list.swap(i, i+1);
                        //         // self.log_message(&format!("todo_item: {todo_item:?} next_item: {next_item:?}"));
                        //     } else if let Some(previous_item) =  {
                        //         // full_list.swap(i, i-1);
                        //         // self.log_message(&format!("todo_item: {todo_item:?} previous_item: {previous_item:?}"));
                        //     }
                        // }
                    }
                }
                
                log::info!("Selected: {:?}", list_state.selected());
                if list_state.selected().is_none() {
                    list_state.select_first();
                } else {
                    list_state.select_next();
                }
                true
            },
            _ => false
        }
    }
}
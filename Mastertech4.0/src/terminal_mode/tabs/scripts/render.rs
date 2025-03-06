use ratatui::{crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind}, layout::{Constraint, Direction, Layout, Margin, Rect, Size}, prelude::{Backend, StatefulWidget}, style::{Style, Stylize}, text::{Line, Span, Text}, widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, WidgetRef}, Frame};
use crate::terminal_mode::{styling::{BASE_COLORS, CATPPUCCIN}, tabs::checklist::TodoItem, widgets::{ButtonType, HandleWidget, ShrinkArea}};
use super::{checklist::Status, ScriptsTab};
use tui_scrollview::ScrollView;

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
    fn draw_log_section<B: Backend>(&self, f: &mut Frame, area: Rect) {
        // 📝 Render logs
        let log_text: Vec<Line> = self.reports.borrow().iter().enumerate().map(|(index, r)| {
            let color = BASE_COLORS[index % BASE_COLORS.len()];
        
            // Extract message and split at " => "
            let (left_text, right_text) = match r.msg.split_once(" => ") {
                Some((left, right)) => (left.trim(), right.trim()), // Trim spaces
                None => (r.msg.as_str(), ""), // If no " => ", treat it as left_text
            };
        
            // Get the available width
            let available_width = area.width as usize;
        
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
        let visible_height = area.height.saturating_sub(2); // Subtract borders
        let virtual_height = log_lines.max(visible_height); // Dynamic height based on content

        // Calculate the maximum line width for horizontal scrolling
        let max_line_width = log_text.iter()
            .map(|line| line.width() as u16) // Width of each line in characters
            .max()
            .unwrap_or(area.width); // Default to visible width if no lines
        let visible_width = area.width.saturating_sub(2); // Subtract borders
        let virtual_width = max_line_width.max(visible_width); // Dynamic width based on longest line

        let virtual_size = Size {
            width: virtual_width,
            height: virtual_height,
        };   

        let mut scroll_view = ScrollView::new(virtual_size);
            
        self.report_area.replace(Some(area));

        let log_widget = Paragraph::new(log_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::new().fg(CATPPUCCIN.blue))
                    .title("Run Report")
                    .border_type(BorderType::Rounded)
            );
    
        scroll_view.render_widget(log_widget, scroll_view.area());
        scroll_view.render(area, f.buffer_mut(), &mut self.report_scroll_state.borrow_mut());
    } 

    fn draw_checklist<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let list_scroll_layout = Layout::horizontal([
            Constraint::Percentage(5), 
            Constraint::Percentage(95)
            ])
            .split(area);

        let list_area = list_scroll_layout[1];
        let scroll_area = list_scroll_layout[0];
        self.checklist_area.replace(Some(list_area));
        self.scroll_area.replace(Some(scroll_area));

        let mut checklist_items: Vec<ListItem> = Vec::new();
        let mut item_to_flat_index: Vec<usize> = Vec::new();
        let mut flat_index = 0;

        for list in self.checklists.values() {
            checklist_items.push(ListItem::new(Line::styled(
                format!("📌 {}", list.name),
                Style::default().fg(CATPPUCCIN.sapphire).bold(),
            )));
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

        self.total_items.replace(checklist_items.len());
        self.visible_height.replace(list_area.height.saturating_sub(2) as usize);

        let checklist = List::new(checklist_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Job Builder")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(CATPPUCCIN.lavender)),
            )
            .highlight_symbol("=> ")
            .highlight_style(Style::new().bg(CATPPUCCIN.base).fg(CATPPUCCIN.teal));

        let mut list_state = self.list_state.borrow_mut();
        f.render_stateful_widget(checklist, list_area, &mut list_state);

        // Update and render the Scrollbar
        let total_items = *self.total_items.borrow();
        let visible_height = *self.visible_height.borrow();
        let scrollable_length = total_items.saturating_sub(visible_height); // Max offset
        let mut scroll_state = self.list_scroll_state.borrow_mut();
        *scroll_state = ScrollbarState::new(scrollable_length); // Set content_length to scrollable range
        *scroll_state = scroll_state.position(list_state.offset());

        let vertical_scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalLeft)
            .begin_symbol(Some("↑ ↑"))
            .track_style(Style::new().fg(CATPPUCCIN.base))
            .track_symbol(Some("║ ║"))
            .thumb_symbol("|▮|")
            .thumb_style(Style::new().fg(CATPPUCCIN.sky)) // .bg(CATPPUCCIN.base)
            .end_symbol(Some("↓ ↓"));

        f.render_stateful_widget(
            vertical_scrollbar, 
            scroll_area.inner(
                Margin {
                    vertical: 1,
                    horizontal: 0,
                }
            ), 
            &mut scroll_state
        );
    }
    
    fn render_popup(&self, f: &mut Frame) {
        if let Some((widget_id, popup_area)) = &*self.active_popup.borrow() {
            let highlighted_idx = *self.popup_highlighted_idx.borrow();
            let submenu_items = match widget_id.0.as_str() {
                "Tuneup" => vec![
                    Span::raw("Run Tuneup").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("View Logs").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "Qc" => vec![
                    Span::raw("Run QC").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Check Status").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "WindowsUpdates" => vec![
                    Span::raw("Check Updates").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Install Now").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "RunPrechecks" => vec![
                    Span::raw("Run Prechecks").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("View Results").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "GetTaskbarItems" => vec![
                    Span::raw("Fetch Items").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Export List").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "GetScheduledTasks" => vec![
                    Span::raw("List Tasks").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Disable Selected").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "GetAntivirus" => vec![
                    Span::raw("Scan Antivirus").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Update Definitions").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "GetInstalledPrograms" => vec![
                    Span::raw("List Programs").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Uninstall").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                "GetStartupItems" => vec![
                    Span::raw("Fetch Startups").style(if highlighted_idx == Some(0) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                    Span::raw("Disable All").style(if highlighted_idx == Some(1) { Style::new().bg(CATPPUCCIN.mauve) } else { Style::default() }),
                ],
                _ => vec![Span::raw("No Options").style(Style::default())],
            };
            
            let lines: Text = submenu_items.into_iter().collect();
            let popup = Paragraph::new(lines)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!("{} Menu", widget_id.0))
                    // .border_style(Style::new().fg(CATPPUCCIN.peach))
                    .style(Style::new().fg(CATPPUCCIN.sky))
                );
            // f.buffer_mut().reset();
            // Clear the area first to overwrite existing content
            f.render_widget(Clear, *popup_area);
            f.render_widget(popup, *popup_area);
        }
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

    fn select_previous_valid(&self, list_state: &mut ListState) {
        let selected = list_state.selected().unwrap_or(0);
        let total_items = *self.total_items.borrow();
        let new_selected = if selected == 0 {
            total_items.saturating_sub(1)
        } else {
            selected.saturating_sub(1)
        };
        let mut current = new_selected;
        while !self.is_valid_item(current) && current != selected {
            current = if current == 0 {
                total_items.saturating_sub(1)
            } else {
                current.saturating_sub(1)
            };
        }
        list_state.select(Some(current));
        let visible_height = *self.visible_height.borrow();
        let offset = list_state.offset();
        if current < offset {
            *list_state.offset_mut() = current;
        } else if current >= offset + visible_height {
            *list_state.offset_mut() = current.saturating_sub(visible_height.saturating_sub(1));
        }
        let mut scroll_state = self.list_scroll_state.borrow_mut();
        *scroll_state = scroll_state.position(list_state.offset());
    }
    
    fn select_next_valid(&self, list_state: &mut ListState) {
        let selected = list_state.selected().unwrap_or(0);
        let total_items = *self.total_items.borrow();
        let new_selected = (selected + 1) % total_items;
        let mut current = new_selected;
        while !self.is_valid_item(current) && current != selected {
            current = (current + 1) % total_items;
        }
        list_state.select(Some(current));
        let visible_height = *self.visible_height.borrow();
        let offset = list_state.offset();
        if current < offset {
            *list_state.offset_mut() = current;
        } else if current >= offset + visible_height {
            *list_state.offset_mut() = current.saturating_sub(visible_height.saturating_sub(1));
        }
        let mut scroll_state = self.list_scroll_state.borrow_mut();
        *scroll_state = scroll_state.position(list_state.offset());
    }

    fn is_valid_item(&self, index: usize) -> bool {
        let (_, item_to_flat_index) = self.build_full_list();
        item_to_flat_index.contains(&index)
    }

    // Ensure selection is not on a header
    fn ensure_valid_selection(&self, list_state: &mut ListState) {
        let (full_list, item_to_flat_index) = self.build_full_list();
        let checklist_items_len = full_list.len() + self.checklists.len(); // Headers + items
        if let Some(selected) = list_state.selected() {
            if selected < checklist_items_len && !item_to_flat_index.contains(&selected) {
                let next_valid = (selected + 1..checklist_items_len)
                    .find(|&i| item_to_flat_index.contains(&i))
                    .or_else(|| (0..selected).rev().find(|&i| item_to_flat_index.contains(&i)));
                list_state.select(next_valid);
            }
        }
    }
}

impl<'a> HandleWidget<'_> for ScriptsTab<'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.receive();

        let mut frame_area = self.frame_area.borrow_mut();
        if frame_area.is_none() {
            *frame_area = Some(f.area());
        }

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

        let para = Paragraph::new("Scripts Library")
            .block(Block::default().bg(CATPPUCCIN.base))
            .centered();

        para.render_ref(left_side_chunks[0], f.buffer_mut());

        // Create grid layout for buttons
        let button_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Ratio(1, 8); 9])
            .split(left_side_chunks[1]);
        
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35),  // Checklist 
                Constraint::Percentage(65),  // Log Messages
            ])
            .split(right_half);

        // Render main buttons
        f.render_widget(&self.tuneup_btn, button_grid[0].shrink(4, 1));
        f.render_widget(&self.qc_btn, button_grid[1].shrink(4, 1));
        f.render_widget(&self.updates_btn, button_grid[2].shrink(4, 1));
        f.render_widget(&self.prechecks_btn, button_grid[3].shrink(4, 1));
        f.render_widget(&self.get_taskbar_items_btn, button_grid[4].shrink(4, 1));
        f.render_widget(&self.get_scheduled_tasks_btn, button_grid[5].shrink(4, 1));
        f.render_widget(&self.get_antivirus_btn, button_grid[6].shrink(4, 1));
        f.render_widget(&self.get_installed_programs_btn, button_grid[7].shrink(4, 1));
        f.render_widget(&self.get_startup_items_btn, button_grid[8].shrink(4, 1));

        // Render log section
        self.draw_log_section::<B>(f, layout[1]);

        // Render Checklist
        self.draw_checklist::<B>(f, layout[0]);

        // Render popup if active
        self.render_popup(f);
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let c = mouse_event.column;
        let r = mouse_event.row;
        match mouse_event.kind {
            MouseEventKind::ScrollDown => {
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
                        let mut list_state = self.list_state.borrow_mut();
                        let current_offset = list_state.offset();
                        let visible_height = *self.visible_height.borrow();
                        let max_offset = self.total_items.borrow().saturating_sub(visible_height);
                        *list_state.offset_mut() = (current_offset + 1).min(max_offset);
                        let mut scroll_state = self.list_scroll_state.borrow_mut();
                        *scroll_state = scroll_state.position(list_state.offset());
                    }
                };
            },
            MouseEventKind::ScrollUp => {
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
                        let mut list_state = self.list_state.borrow_mut();
                        let current_offset = list_state.offset();
                        *list_state.offset_mut() = current_offset.saturating_sub(1);
                        let mut scroll_state = self.list_scroll_state.borrow_mut();
                        *scroll_state = scroll_state.position(list_state.offset());
                    }
                };
            },
            MouseEventKind::ScrollLeft => self.report_scroll_state.borrow_mut().scroll_left(),
            MouseEventKind::ScrollRight => self.report_scroll_state.borrow_mut().scroll_right(),
            _ => {
                if let Some(scroll_area) = *self.scroll_area.borrow() {
                    if c >= scroll_area.x && c < scroll_area.x + scroll_area.width &&
                        r >= scroll_area.y && r < scroll_area.y + scroll_area.height 
                    {
                        if let MouseEventKind::Drag(MouseButton::Left) = mouse_event.kind {
                            let click_row = (r - scroll_area.y) as usize;
                            let scroll_area_height = scroll_area.height as usize;
                            let total_items = *self.total_items.borrow();
                            let visible_height = *self.visible_height.borrow();
                            let scrollable_length = total_items.saturating_sub(visible_height);
                            let new_offset = (click_row * scrollable_length) / scroll_area_height;
                            let mut list_state = self.list_state.borrow_mut();
                            let max_offset = scrollable_length;
                            *list_state.offset_mut() = new_offset.min(max_offset);
                            let mut scroll_state = self.list_scroll_state.borrow_mut();
                            *scroll_state = scroll_state.position(list_state.offset());
                        }
                    }
                }

                let buttons = [
                    &self.tuneup_btn,
                    &self.qc_btn,
                    &self.updates_btn,
                    &self.prechecks_btn,
                    &self.get_taskbar_items_btn,
                    &self.get_scheduled_tasks_btn,
                    &self.get_antivirus_btn,
                    &self.get_installed_programs_btn,
                    &self.get_startup_items_btn,
                ];

                for button in buttons.iter() {
                    if let Some(area) = button.get_area() {
                        let in_area = c >= area.x 
                            && c < area.x + area.width 
                            && r >= area.y 
                            && r < area.y + area.height;
                            
                        if let MouseEventKind::Down(MouseButton::Left) = mouse_event.kind {
                            if in_area {    
                                break;
                            } else {
                                self.active_popup.replace(None);
                                self.popup_highlighted_idx.replace(None);
                                break;
                            }
                        }
                    }
                }

                if let MouseEventKind::Moved = mouse_event.kind {
                    if let Some((widget_id, popup_area)) = &*self.active_popup.borrow() {
                        if c >= popup_area.x && c < popup_area.x + popup_area.width &&
                           r >= popup_area.y && r < popup_area.y + popup_area.height {
                            let relative_row = (r - (popup_area.y + 1)) as usize; // Adjust for top border
                            let span_count = match widget_id.0.as_str() {
                                "Tuneup" | "Qc" | "WindowsUpdates" | "RunPrechecks" |
                                "GetTaskbarItems" | "GetScheduledTasks" | "GetAntivirus" |
                                "GetInstalledPrograms" | "GetStartupItems" => 2,
                                _ => 1,
                            };
                            if relative_row < span_count {
                                log::info!("Hovering over Span {} at row {}", relative_row, r);
                                self.popup_highlighted_idx.replace(Some(relative_row));
                            } else {
                                self.popup_highlighted_idx.replace(None);
                            }
                        } else {
                            self.popup_highlighted_idx.replace(None);
                        }
                    }
                }

                if let MouseEventKind::Down(MouseButton::Left) = mouse_event.kind {
                    // Handle clicks within popup
                    if let Some((widget_id, popup_area)) = &*self.active_popup.borrow() {
                        if c >= popup_area.x && c < popup_area.x + popup_area.width &&
                           r >= popup_area.y && r < popup_area.y + popup_area.height {
                            // Calculate which Span was clicked (each Span is one line)
                            let relative_row = (r - popup_area.y) as usize;
                            let span_count = match widget_id.0.as_str() {
                                "Tuneup" | "Qc" | "WindowsUpdates" | "RunPrechecks" |
                                "GetTaskbarItems" | "GetScheduledTasks" | "GetAntivirus" |
                                "GetInstalledPrograms" | "GetStartupItems" => 2,
                                _ => 1,
                            };
                            if relative_row < span_count {
                                log::info!("Clicked Span {} in popup", relative_row);
                                // Example action: Log or open submenu
                                match widget_id.0.as_str() {
                                    "Tuneup" => match relative_row {
                                        0 => self.log_message("Run Tuneup clicked"),
                                        1 => self.log_message("View Logs clicked"),
                                        _ => {},
                                    },
                                    "Qc" => match relative_row {
                                        0 => self.log_message("Run QC clicked"),
                                        1 => self.log_message("Check Status clicked"),
                                        _ => {},
                                    },
                                    // Add more cases as needed
                                    _ => {},
                                }
                            }
                        }
                    }
                }

                self.tuneup_btn.handle_mouse_event(&mouse_event);
                self.qc_btn.handle_mouse_event(&mouse_event);
                self.prechecks_btn.handle_mouse_event(&mouse_event);
                self.updates_btn.handle_mouse_event(&mouse_event);
                self.get_antivirus_btn.handle_mouse_event(&mouse_event);
                self.get_installed_programs_btn.handle_mouse_event(&mouse_event);
                self.get_startup_items_btn.handle_mouse_event(&mouse_event);
                self.get_scheduled_tasks_btn.handle_mouse_event(&mouse_event);
                self.get_taskbar_items_btn.handle_mouse_event(&mouse_event);
                // for (_id, button) in self.tab_buttons.iter() { button.handle_mouse_event(&mouse_event); }
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
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
                let selected = list_state.selected();
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    drop(list_state);
                    if let Some(selected) = selected {
                        self.move_item(MoveDirection::Up, selected);
                    }
                } else {
                    if selected.is_none() {
                        self.select_first_valid(&mut list_state);
                    } else {
                        self.select_previous_valid(&mut list_state);
                    }
                    // Ensure header skipping after movement
                    self.ensure_valid_selection(&mut list_state);
                }
                true
            }
            KeyCode::Down => {
                log::info!("DOWN");
                let mut list_state = self.list_state.borrow_mut();
                let selected = list_state.selected();
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    self.log_message(&format!("CTRL DOWN: {key_event:?}"));
                    drop(list_state);
                    if let Some(selected) = selected {
                        self.move_item(MoveDirection::Down, selected);
                    }
                } else {
                    if selected.is_none() {
                        self.select_first_valid(&mut list_state);
                    } else {
                        self.select_next_valid(&mut list_state);
                    }
                    // Ensure header skipping after movement
                    self.ensure_valid_selection(&mut list_state);
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
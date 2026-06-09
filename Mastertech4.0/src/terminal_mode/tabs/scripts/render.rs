use ratatui::{crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind}, layout::{Alignment, Constraint, Direction, Layout, Margin, Position, Rect}, prelude::Backend, style::{Style, Stylize}, text::{Line, Span}, widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Widget, Wrap}, Frame};
use crate::terminal_mode::{events::action_handler::WidgetId, fx::effect::animated_border, styling::{CATPPUCCIN, APP_BACKGROUND, THEME}, tabs::checklist::TodoItem, widgets::{ButtonType, HandleWidget, ShrinkArea}};
use super::{checklist::Status, ScriptsTab};
use displays::get_current_user_from_auth;
use unicode_width::UnicodeWidthStr;

#[cfg(target_os="windows")]
use {
    crate::terminal_mode::tabs::scripts::script_categories::{format_size, get_directory_size},
    std::path::Path
};

#[derive(Clone, Debug)]
pub struct Report {
    pub reporter: Reporter,
    pub msg: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reporter {
    Tuneup,
    RunPrechecks,
    Informational,
    JunkwareRemoval,
    UserScript,
    Robocopy,
    StressTest,
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
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(THEME.accent))
            .title("Run Report")
            .title_style(THEME.title())
            .border_type(BorderType::Rounded);

        let outer_area = area;

        let list_scroll_layout = Layout::horizontal([
            Constraint::Percentage(4), 
            Constraint::Percentage(96)
            ])
            .split(block.inner(outer_area));

        let main_area = list_scroll_layout[1];
        let v_scroll_area = list_scroll_layout[0];

        let main_content = Layout::vertical([
            Constraint::Percentage(97),
            Constraint::Percentage(3), 
            ])
            .split(main_area);

        let h_scroll_area = main_content[1];
        let inner_area = main_content[0];

        (&block).render(outer_area, f.buffer_mut());
        // Check if robocopy logs exist
        let has_robocopy_logs = !self.robocopy_reports.borrow().is_empty();

        // Combine regular and robocopy logs
        let mut log_text: Vec<Line> = Vec::new();

        // Add regular logs
        log_text.extend(self.reports.borrow().iter().map(|r| {
            let (left_text, right_text) = match r.msg.split_once(" => ") {
                Some((left, right)) => (left.trim().to_string(), right.trim().to_string()),
                None => (r.msg.clone(), String::new()),
            };
            let available_width = inner_area.width as usize;
            let reporter_text = format!("{:?} => ", r.reporter);
            let width = available_width.saturating_sub(left_text.len() + reporter_text.len());

            Line::from(vec![
                Span::styled(reporter_text, Style::default().fg(THEME.tertiary)),
                Span::styled(left_text, Style::default().fg(THEME.text)),
                Span::styled(format!(" {:>width$}", right_text, width = width), Style::default().fg(THEME.text_muted)),
            ])
        }));

        // Add robocopy logs (up to ROBOCOPY_DISPLAY_LINES) if present
        if has_robocopy_logs {
            let robocopy_logs: Vec<Line> = self.robocopy_reports.borrow().iter().rev() // Latest first
                .take(Self::ROBOCOPY_DISPLAY_LINES)
                .rev() // Restore order
                .map(|r| {
                    Line::from(vec![
                        Span::styled("Robocopy: ", Style::default().fg(THEME.tertiary)),
                        Span::styled(r.msg.clone(), Style::default().fg(THEME.text)),
                    ])
                })
                .collect();
            log_text.extend(robocopy_logs);
        }

        let log_lines = log_text.len() as u16;
        let visible_height = inner_area.height; // Full area height
        let max_line_width = log_text.iter()
            .map(|line| line.width() as u16)
            .max()
            .unwrap_or(inner_area.width);

        self.report_area.replace(Some(inner_area));

        let mut scroll_state = self.report_scroll_state.borrow_mut();
        // Clamp scroll_y and scroll_x to content bounds
        let scroll_y = scroll_state.offset().y.min(log_lines.saturating_sub(visible_height));
        let scroll_x = scroll_state.offset().x.min(max_line_width.saturating_sub(inner_area.width));
        scroll_state.set_offset(Position { x: scroll_x, y: scroll_y });

        // Adjust scroll_y to ensure robocopy logs are visible when present
        let effective_visible_height = if has_robocopy_logs {
            visible_height.saturating_sub(Self::ROBOCOPY_DISPLAY_LINES as u16)
        } else {
            visible_height
        };

        let start_line = if has_robocopy_logs && log_lines > visible_height {
            // Ensure the last ROBOCOPY_DISPLAY_LINES are visible
            let robocopy_count = log_text.len().saturating_sub(self.reports.borrow().len()).min(Self::ROBOCOPY_DISPLAY_LINES) as u16;
            let regular_lines = log_lines.saturating_sub(robocopy_count);
            let max_scroll = regular_lines.saturating_sub(effective_visible_height);
            scroll_y.min(max_scroll)
        } else {
            scroll_y
        };

        let end_line = (start_line + visible_height).min(log_lines) as usize;
        let visible_lines = if start_line < log_text.len() as u16 {
            &log_text[start_line as usize..end_line]
        } else {
            &[]
        };

        let log_widget = Paragraph::new(visible_lines.to_vec())
            .scroll((0, scroll_x)) // Only horizontal scroll here
            .alignment(Alignment::Left);

        f.render_widget(log_widget, inner_area);

        // Vertical Scrollbar
        if log_lines > visible_height {
            let mut v_scrollbar_state = ScrollbarState::new(log_lines.saturating_sub(visible_height) as usize);
            v_scrollbar_state = v_scrollbar_state.position(start_line as usize);

            let v_scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalLeft)
                .begin_symbol(Some("🢁"))
                .track_style(Style::new().fg(THEME.surface))
                .track_symbol(Some("║║"))
                .thumb_symbol("⦕⦖")
                .thumb_style(Style::new().fg(THEME.accent))
                .end_symbol(Some("🢃"));

            f.render_stateful_widget(
                v_scrollbar,
                v_scroll_area,
                &mut v_scrollbar_state,
            );
        }

        // Horizontal Scrollbar
        if max_line_width > inner_area.width {
            let mut h_scrollbar_state = ScrollbarState::new(max_line_width.saturating_sub(inner_area.width) as usize);
            h_scrollbar_state = h_scrollbar_state.position(scroll_x as usize);

            let h_scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                .begin_symbol(Some("⟸"))
                .track_style(Style::new().fg(THEME.surface))
                .track_symbol(Some("⥈"))
                .thumb_symbol("|⟗|")
                .thumb_style(Style::new().fg(THEME.accent))
                .end_symbol(Some("⟹"));
            f.render_stateful_widget(
                h_scrollbar,
                h_scroll_area,
                &mut h_scrollbar_state,
            );
        }
    }

    /// Collapsed Job Builder: a thin pink spine with a vertical label. Click to expand.
    fn draw_job_builder_spine(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(THEME.accent));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let label = "\u{25b8} JOB BUILDER";
        for (i, ch) in label.chars().enumerate() {
            let yy = inner.y + i as u16;
            if yy >= inner.y + inner.height {
                break;
            }
            let style = if i == 0 {
                Style::new().fg(THEME.accent)
            } else {
                Style::new().fg(THEME.tertiary)
            };
            f.buffer_mut().set_string(inner.x, yy, ch.to_string(), style);
        }
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

        for list in Self::CHECKLIST_ORDERED {
            if let Some(list) = self.checklists.get(list) {
                checklist_items.push(
                    ListItem::new(
                    Line::styled(
                        format!("* {} *", list.name),
                        THEME.title(),
                        )
                    )
                );

                for item in &list.items {
                    let symbol = match item.status {
                        Status::Completed => "\u{25fc}", // ◼
                        Status::Todo => "\u{25fb}",       // ◻
                    };

                    let mut style = match item.status {
                        Status::Completed => Style::default().fg(THEME.accent),
                        Status::Todo => Style::default().fg(THEME.tertiary),
                    };

                    if let Some((current_cat, current_text)) = &*self.current_script.borrow() {
                        if *current_cat == item.category() && *current_text == item.text {
                            style = Style::new().bg(THEME.surface).fg(THEME.text);
                        }
                    }

                    checklist_items.push(ListItem::new(Line::styled(
                        format!("{} {}", symbol, item.text),
                        style,
                    )));
                    item_to_flat_index.push(flat_index);
                    flat_index += 1;
                }
            }
        }

        self.total_items.replace(checklist_items.len());
        self.visible_height.replace(list_area.height.saturating_sub(2) as usize);

        let checklist = List::new(checklist_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Job Builder")
                    .title_style(THEME.title())
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(THEME.tertiary)),
            )
            .highlight_symbol("\u{25b8} ")
            .highlight_style(THEME.menu_highlight());

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
            .track_style(Style::new().fg(THEME.surface))
            .track_symbol(Some("║ ║"))
            .thumb_symbol("|▮|")
            .thumb_style(Style::new().fg(THEME.accent)) // .bg(CATPPUCCIN.base)
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
    
    fn draw_data_path_buttons<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let popup_title = "  Data Transfer Options  ";
        let popup_text = "Please choose a destination for the data transfer, and right click on a path to exclude it from the data transfer";

        // Calculate button grid dimensions
        let button_count = self.data_path_buttons.len();
        let rows = (button_count + 1) / 2; // 2 columns

        let max_width = self.data_path_buttons
            .iter()
            .map(|path| path.get_label().width() as u16)
            .max()
            .unwrap_or(popup_text.width() as u16);

        let popup_width: u16 = (max_width * 2) + 10; 
        let inner_width = popup_width.saturating_sub(2); // Account for margins
        let text_lines = (popup_text.len() as u16 + inner_width - 1) / inner_width; // Ceiling division
        let text_height = text_lines.max(2); // Ensure at least 2 lines, adjust as needed
    
        // Calculate popup height
        let popup_height = text_height + 2 + rows as u16 * 5 + 2; // Text + padding + buttons + borders

        // Center the popup in the provided area
        let popup_area = Rect::new(
            (area.width.saturating_sub(popup_width)) / 2 + area.x,
            (area.height.saturating_sub(popup_height)) / 2 + area.y,
            popup_width.min(area.width),
            popup_height.min(area.height),
        );

        // Clear the background
        f.render_widget(Clear, popup_area);

        // Create a centered block as the container
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(popup_title)
            .title_alignment(Alignment::Center)
            .title_style(THEME.title())
            .border_style(Style::default().fg(THEME.accent))
            .style(Style::default().bg(APP_BACKGROUND).fg(THEME.text));

        f.render_widget(block, popup_area);

        // Define inner area for content
        let inner_area = popup_area.inner(Margin { horizontal: 1, vertical: 1 });

        // Split inner area: text at top, buttons below
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(text_height), // Text area with padding
                Constraint::Length(8), // Padding
                Constraint::Min(rows as u16 * 3), // Buttons
                Constraint::Length(3), // Custom_path_field
                Constraint::Length(6), // Custom_path_field
            ])
            .split(inner_area);

        // Render the instructional text
        let text_block = Paragraph::new(popup_text)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);

        f.render_widget(text_block, content_chunks[0]);

        // Button grid within the lower chunk
        let button_area = content_chunks[2];
        let button_constraints: Vec<Constraint> = vec![Constraint::Length(3); rows];
        let button_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints(button_constraints)
            .split(button_area);

        for (i, button) in self.data_path_buttons.iter().enumerate() {
            let row = i / 2;
            let col = i % 2;
            let col_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(button_grid[row]);

            // f.render_widget(Clear, popup_area);
            f.render_widget(button, col_chunks[col].shrink(1, 0));
        }

        f.render_widget(&self.custom_path_field, content_chunks[4].shrink(1, 0));
        // self.custom_destination_field.render_ref(content_chunks[4].shrink(1, 0), f.buffer_mut());

    }

    fn draw_context_menu(&self, f: &mut Frame) {
        if let Some((widget_id, popup_area)) = &*self.active_popup.borrow() {
            let items = self.popup_items.borrow();
            let items = items.get(&widget_id.0);
            let list_items: Vec<ListItem> = items.map_or(
                vec![ListItem::new("No Options")],
                |items| {
                    items.iter().map(|item| {
                        let (marker, color) = match item.status {
                            Status::Todo => ("\u{25fb} ", THEME.tertiary),
                            Status::Completed => ("\u{25fc} ", THEME.accent),
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(marker, Style::new().fg(color)),
                            Span::styled(item.text.clone(), Style::new().fg(THEME.text)),
                        ]))
                    }).collect()
                }
            );

            let popup = List::new(list_items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!("{} Menu", widget_id.0))
                    .title_style(THEME.title())
                    .border_style(Style::new().fg(THEME.accent))
                    .title_alignment(ratatui::layout::Alignment::Center)
                    .style(Style::new().bg(APP_BACKGROUND).fg(THEME.text))
                )
                .highlight_style(THEME.menu_highlight())
                .highlight_symbol("\u{25b8} ");

            f.render_widget(Clear, *popup_area);
            let mut popup_state = self.popup_list_state.borrow_mut();
            f.render_stateful_widget(popup, *popup_area, &mut popup_state);
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
        // Track area and offset for mouse coordinate adjustment
        let total_offset = area.y;
        self.total_offset.replace(total_offset);
        self.scripts_area.replace(Some(area));
        
        self.process_mcp_requests();
        self.receive();
        self.process_mcp_completions();
        if self.filesystem.receive() {
            self.user_scripts_bucket_loaded = true;
        }
        self.insert_user_scripts();

        let mut init = self.init.borrow_mut();
        if *init {
            if self.filesystem.user.get_name().len() > 0 {
                log::info!("We have a user, requesting contents");
                let _ = self.filesystem.request_contents("/");
                self.user_scripts_bucket_loaded = false;
                self.check_for_scripts = true;
            } else {
                log::info!("We need a user");
                let user = get_current_user_from_auth();
                match user {
                    Some(usr) => {
                        let _ = self.filesystem.set_user(usr);
                        let _ = self.filesystem.request_contents("/");
                        self.user_scripts_bucket_loaded = false;
                        self.check_for_scripts = true;
                    },
                    None => log::info!("Could not retrieve user."),
                };
            }
            *init = false;
        }
        
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

        const SCRIPT_BUTTONS_ROW_HEIGHT: u16 = 3;
        // Action slots below the category buttons: service_number, current_script,
        // progress, update progress, fs progress, transfers (two slots), run, stress.
        const SCRIPT_BUTTONS_SLOT_COUNT: u16 = 9;
        const SCRIPT_BUTTONS_VIRTUAL_HEIGHT: u16 = SCRIPT_BUTTONS_SLOT_COUNT * SCRIPT_BUTTONS_ROW_HEIGHT;
        const CATEGORY_HEIGHT: u16 = 12; // 4 framed buttons, 3 rows each

        let left_side_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),               // title
                Constraint::Length(CATEGORY_HEIGHT), // compact framed category buttons
                Constraint::Fill(1),                 // action widgets
            ])
            .split(left_half);

        let para = Paragraph::new("Scripts Library")
            .block(Block::default().bg(APP_BACKGROUND))
            .centered();

        (&para).render(left_side_chunks[0], f.buffer_mut());

        // Compact framed category buttons, tightly stacked.
        let cat_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(left_side_chunks[1]);
        f.render_widget(&self.tuneup_btn, cat_rows[0].shrink_symmetric(1, 0));
        f.render_widget(&self.informational_btn, cat_rows[1].shrink_symmetric(1, 0));
        f.render_widget(&self.user_scripts_btn, cat_rows[2].shrink_symmetric(1, 0));
        f.render_widget(&self.stress_tests_btn, cat_rows[3].shrink_symmetric(1, 0));

        let buttons_area = left_side_chunks[2];
        let needs_scroll = buttons_area.height < SCRIPT_BUTTONS_VIRTUAL_HEIGHT;

        // Only allocate a scrollbar column when content overflows the viewport
        let (viewport_rect, script_scrollbar_rect) = if needs_scroll {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(2), Constraint::Fill(1)])
                .split(buttons_area);
            (cols[1], Some(cols[0]))
        } else {
            (buttons_area, None)
        };

        // Clamp scroll offset (0 when everything fits)
        let max_offset = SCRIPT_BUTTONS_VIRTUAL_HEIGHT.saturating_sub(viewport_rect.height);
        let mut scroll_offset = *self.script_buttons_scroll_offset.borrow();
        scroll_offset = scroll_offset.min(max_offset);
        self.script_buttons_scroll_offset.replace(scroll_offset);
        self.script_buttons_viewport.replace(Some(viewport_rect));

        // Build slot rects: each slot has virtual height SCRIPT_BUTTONS_ROW_HEIGHT
        let mut slot_rects: Vec<Rect> = Vec::with_capacity(SCRIPT_BUTTONS_SLOT_COUNT as usize);
        if needs_scroll {
            for i in 0..SCRIPT_BUTTONS_SLOT_COUNT {
                let row_top = i * SCRIPT_BUTTONS_ROW_HEIGHT;
                let row_bottom = row_top + SCRIPT_BUTTONS_ROW_HEIGHT;
                // Clip to the visible window [scroll_offset .. scroll_offset + viewport_height]
                let vis_top = row_top.max(scroll_offset);
                let vis_bot = row_bottom.min(scroll_offset + viewport_rect.height);
                if vis_bot <= vis_top {
                    slot_rects.push(Rect::default());
                    continue;
                }
                let screen_y = viewport_rect.y + (vis_top - scroll_offset);
                let vis_h = vis_bot - vis_top;
                slot_rects.push(Rect {
                    x: viewport_rect.x,
                    y: screen_y,
                    width: viewport_rect.width,
                    height: vis_h,
                });
            }
        } else {
            // Everything fits — give each slot its full row height inside the viewport
            let per_slot = viewport_rect.height / SCRIPT_BUTTONS_SLOT_COUNT;
            let remainder = viewport_rect.height % SCRIPT_BUTTONS_SLOT_COUNT;
            let mut y = viewport_rect.y;
            for i in 0..SCRIPT_BUTTONS_SLOT_COUNT {
                let h = per_slot + if i < remainder { 1 } else { 0 };
                slot_rects.push(Rect { x: viewport_rect.x, y, width: viewport_rect.width, height: h });
                y += h;
            }
        }

        let job_builder_collapsed = *self.job_builder_collapsed.borrow();
        let (checklist_area, log_area, jb_toggle_area) = if job_builder_collapsed {
            let cols = Layout::horizontal([Constraint::Length(5), Constraint::Fill(1)]).split(right_half);
            (cols[0], cols[1], cols[0])
        } else {
            let cols = Layout::horizontal([Constraint::Percentage(28), Constraint::Percentage(72)]).split(right_half);
            // The title row of the checklist toggles the collapse.
            let title = Rect { x: cols[0].x, y: cols[0].y, width: cols[0].width, height: 1 };
            (cols[0], cols[1], title)
        };
        self.job_builder_toggle_area.replace(Some(jb_toggle_area));

        // Shrink helper: only shrink when rect height >= 3 so we don't give widgets height < 3
        let shrink_slot = |r: Rect, px: u16, py: u16| {
            if r.height >= 3 { r.shrink(px, py) } else { r }
        };

        // Render action widgets (only when slot has non-zero height)
        if slot_rects[0].height > 0 {
            // Cap the input to a compact 3-row box so a tall slot doesn't stretch it.
            let sn_slot = Rect { height: slot_rects[0].height.min(3), ..slot_rects[0] };
            f.render_widget(&self.service_number_field, sn_slot.shrink_symmetric(2, 0));
        }

        let current_script = self.current_script.borrow().clone();
        let script_name = &mut String::new();
        if let Some((_, script)) = current_script {
            *script_name = script.clone();
        }
        let script_textarea = Paragraph::new(script_name.clone())
            .alignment(Alignment::Center)
            .centered()
            .block(
                Block::default()
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(THEME.accent))
            );

        if slot_rects[1].height > 0 {
            f.render_widget(script_textarea, shrink_slot(slot_rects[1], 4, 1));
        }

        let mut progress_mut = self.progress.borrow_mut();
        let mut update_progress_mut = self.update_progress.borrow_mut();
        if let Some(progress) = *progress_mut {
            let gauge = Gauge::default()
                .block(Block::bordered().title(format!("{script_name} Progress")))
                .gauge_style(Style::new().fg(THEME.accent).bg(THEME.surface))
                .ratio(progress.0 as f64 / progress.1 as f64);

            if slot_rects[2].height > 0 {
                f.render_widget(&gauge, shrink_slot(slot_rects[2], 2, 1));
            }

            if progress.0 == progress.1 {
                *progress_mut = None;
            }
        } else if let Some(update_progress) = *update_progress_mut {
            let mut install = self.windows_installation.borrow_mut();
            let title = if *install {
                "Windows update install %"
            } else {
                "Windows update download %"
            };

            let gauge = Gauge::default()
                .block(Block::bordered().title(title))
                .gauge_style(Style::new().fg(THEME.accent).bg(THEME.surface))
                .ratio(update_progress as f64 / 100.0);

            if slot_rects[3].height > 0 {
                f.render_widget(&gauge, shrink_slot(slot_rects[3], 2, 1));
            }

            if update_progress == 100 {
                *update_progress_mut = None;
                *install = false;
            }
        } else {
            let total = self.filesystem.total_size;
            let progress = self.filesystem.progress;
            if total != u64::MAX as f32 && total > 0.0 {
                let ratio = if total > 0.0 {
                    (progress / total).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let gauge = Gauge::default()
                    .block(Block::bordered().title(format!("{script_name} Progress")))
                    .gauge_style(Style::new().fg(THEME.accent).bg(THEME.surface))
                    .ratio(ratio as f64);

                if slot_rects[4].height > 0 {
                    f.render_widget(&gauge, shrink_slot(slot_rects[4], 2, 1));
                }
            }
        }

        if slot_rects[5].height > 0 {
            if self.loading {
                let throbber = crate::terminal_mode::widgets::throbber_widgets_tui::Throbber::default()
                    .label("Scanning Directories..")
                    .throbber_set(crate::terminal_mode::widgets::throbber_widgets_tui::VERTICAL_BLOCK);
                f.render_widget(throbber, shrink_slot(slot_rects[5], 0, 1));
            } else {
                let active_processes = self.active_robocopy_processes.borrow();
                if !active_processes.is_empty() {
                    let mut transfer_lines: Vec<Line> = Vec::new();
                    for (pid, progress) in active_processes.iter() {
                        let source_name = std::path::Path::new(&progress.source)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&progress.source);

                        let line = Line::from(vec![
                            Span::styled(
                                format!("[{}] ", pid),
                                Style::default().fg(CATPPUCCIN.mauve)
                            ),
                            Span::styled(
                                format!("{}: ", source_name),
                                Style::default().fg(CATPPUCCIN.blue)
                            ),
                            Span::styled(
                                format!("R:{:.1} MB/s ", progress.bytes_read),
                                Style::default().fg(CATPPUCCIN.green)
                            ),
                            Span::styled(
                                format!("W:{:.1} MB/s", progress.bytes_written),
                                Style::default().fg(CATPPUCCIN.peach)
                            ),
                        ]);
                        transfer_lines.push(line);
                    }

                    let transfer_count = active_processes.len();
                    drop(active_processes);

                    let transfer_block = Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(format!(" Active Transfers ({}) ", transfer_count))
                        .title_style(THEME.title())
                        .border_style(Style::new().fg(THEME.tertiary));

                    let transfer_list = Paragraph::new(transfer_lines)
                        .block(transfer_block)
                        .wrap(Wrap { trim: false });

                    let combined_height = slot_rects[5].height.saturating_add(slot_rects[6].height);
                    let combined_area = if slot_rects[6].height > 0 && combined_height >= 2 {
                        Rect::new(
                            slot_rects[5].x,
                            slot_rects[5].y,
                            slot_rects[5].width,
                            combined_height,
                        )
                    } else {
                        slot_rects[5]
                    };
                    f.render_widget(transfer_list, shrink_slot(combined_area, 0, 1));
                }
            }
        }

        self.run_btn.set_disabled(self.run_button_should_be_disabled());
        if slot_rects[7].height > 0 {
            f.render_widget(&self.run_btn, shrink_slot(slot_rects[7], 4, 1));
        }

        // Quick single-stressor cycle button kept for live throughput preview;
        // the Stress Tests category button opens the catalog popup.
        if slot_rects[8].height > 0 {
            self.stress_test_btn.set_disabled(self.stress_run.borrow().is_some());
            f.render_widget(&self.stress_test_btn, shrink_slot(slot_rects[8], 4, 1));
        }

        // Script buttons column scrollbar — only when content overflows
        if let Some(sb_rect) = script_scrollbar_rect {
            let mut scroll_state = self.script_buttons_scroll_state.borrow_mut();
            *scroll_state = scroll_state
                .content_length(SCRIPT_BUTTONS_VIRTUAL_HEIGHT as usize)
                .position(scroll_offset as usize)
                .viewport_content_length(viewport_rect.height as usize);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalLeft)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));
            f.render_stateful_widget(scrollbar, sb_rect, &mut *scroll_state);
        }

        // Render log section
        self.draw_log_section::<B>(f, log_area);

        // Render the Job Builder (collapsed spine or full checklist)
        if job_builder_collapsed {
            self.draw_job_builder_spine(f, checklist_area);
        } else {
            self.draw_checklist::<B>(f, checklist_area);
        }

        // Add animated border effects to the sections. Reset on collapse toggle
        // (handled in the mouse handler) so the borders track the new areas.
        {
            let mut effects_init = self.effects_init.borrow_mut();
            let mut effect_stage = self.effect_stage.borrow_mut();

            if !*effects_init {
                // Run Report panel border: pink.
                let log_effect = animated_border(THEME.accent, log_area, 20.0);
                effect_stage.add_effect(log_effect);

                // Job Builder panel border: mauve when open, pink spine when collapsed.
                let cl_color = if job_builder_collapsed { THEME.accent } else { THEME.tertiary };
                let checklist_effect = animated_border(cl_color, checklist_area, 18.0);
                effect_stage.add_effect(checklist_effect);

                *effects_init = true;
            }

            // Process effects
            let fx_duration = tachyonfx::Duration::from_millis(16);
            effect_stage.process_effects(fx_duration, f.buffer_mut(), area);
        }

        // Render popup if active
        self.draw_context_menu(f);

        // Check if data_path_buttons has items and draw popup
        let is_open = *self.is_popup_open.borrow();
        if is_open {
            self.draw_data_path_buttons::<B>(f, area);
        }
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // let start_total = Instant::now();
        let c = mouse_event.column;
        let r = mouse_event.row;
        let mouse_position = Position::new(c, r);

        self.service_number_field.handle_mouse_event(&mouse_event);

        // Job Builder collapse toggle — consume the click and re-anchor effects.
        if let MouseEventKind::Down(MouseButton::Left) = mouse_event.kind {
            if let Some(toggle) = *self.job_builder_toggle_area.borrow() {
                if toggle.contains(mouse_position) {
                    let collapsed = *self.job_builder_collapsed.borrow();
                    self.job_builder_collapsed.replace(!collapsed);
                    *self.effect_stage.borrow_mut() = crate::terminal_mode::fx::EffectStage::default();
                    *self.effects_init.borrow_mut() = false;
                    return;
                }
            }
        }

        match mouse_event.kind {
            MouseEventKind::ScrollDown => {
                if let Some(viewport) = *self.script_buttons_viewport.borrow() {
                    if viewport.contains(mouse_position) {
                        const SCRIPT_BUTTONS_VIRTUAL_HEIGHT: u16 = 9 * 3;
                        let max_offset = SCRIPT_BUTTONS_VIRTUAL_HEIGHT.saturating_sub(viewport.height);
                        let mut offset = *self.script_buttons_scroll_offset.borrow();
                        offset = (offset + 1).min(max_offset);
                        self.script_buttons_scroll_offset.replace(offset);
                    }
                }
                if let Some(report_area) = *self.report_area.borrow() {
                    let report_area_contains_mouse = report_area.contains(mouse_position);
                    if report_area_contains_mouse {
                        self.report_scroll_state.borrow_mut().scroll_down();
                        self.report_scroll_state.borrow_mut().scroll_down();
                        *self.has_scrolled_manually.borrow_mut() = true;
                    }
                }
                if let Some(list_area) = *self.checklist_area.borrow() {
                    let list_area_contains_mouse = list_area.contains(mouse_position);
                    if list_area_contains_mouse {
                        let mut list_state = self.list_state.borrow_mut();
                        let current_offset = list_state.offset();
                        let visible_height = *self.visible_height.borrow();
                        let total_items = *self.total_items.borrow();
                        let max_offset = total_items.saturating_sub(visible_height);
                        let new_offset = (current_offset + 1).min(max_offset);
                        *list_state.offset_mut() = new_offset;
    
                        // CHANGED: Adjust selected index to follow scroll
                        if let Some(selected) = list_state.selected() {
                            let new_selected = (selected + 1).min(total_items.saturating_sub(1));
                            if new_selected >= new_offset && new_selected < new_offset + visible_height {
                                list_state.select(Some(new_selected));
                            } else if new_selected < new_offset {
                                list_state.select(Some(new_offset));
                            }
                        } else if total_items > 0 {
                            // If nothing selected, select first visible item after scroll
                            list_state.select(Some(new_offset));
                        }
    
                        let mut scroll_state = self.list_scroll_state.borrow_mut();
                        *scroll_state = scroll_state.position(list_state.offset());
                    }
                }
                
                // log::info!("ScrollDown duration: {:?}", start_scroll_down.elapsed());
            },
            MouseEventKind::ScrollUp => {
                if let Some(viewport) = *self.script_buttons_viewport.borrow() {
                    if viewport.contains(mouse_position) {
                        let mut offset = *self.script_buttons_scroll_offset.borrow();
                        offset = offset.saturating_sub(1);
                        self.script_buttons_scroll_offset.replace(offset);
                    }
                }
                if let Some(report_area) = *self.report_area.borrow() {
                    let report_area_contains_mouse = report_area.contains(mouse_position);
                    if report_area_contains_mouse {
                        self.report_scroll_state.borrow_mut().scroll_up();
                        self.report_scroll_state.borrow_mut().scroll_up();
                        *self.has_scrolled_manually.borrow_mut() = true;
                    }
                }
                if let Some(list_area) = *self.checklist_area.borrow() {
                    let list_area_contains_mouse = list_area.contains(mouse_position);
                    if list_area_contains_mouse {
                        let mut list_state = self.list_state.borrow_mut();
                        let current_offset = list_state.offset();
                        let visible_height = *self.visible_height.borrow();
                        let new_offset = current_offset.saturating_sub(1);
                        *list_state.offset_mut() = new_offset;
    
                        // CHANGED: Adjust selected index to follow scroll
                        if let Some(selected) = list_state.selected() {
                            let new_selected = selected.saturating_sub(1);
                            if new_selected >= new_offset && new_selected < new_offset + visible_height {
                                list_state.select(Some(new_selected));
                            } else if new_selected >= new_offset + visible_height {
                                list_state.select(Some(new_offset + visible_height.saturating_sub(1)));
                            }
                        } else if *self.total_items.borrow() > 0 {
                            // If nothing selected, select last visible item after scroll
                            list_state.select(Some(new_offset));
                        }
    
                        let mut scroll_state = self.list_scroll_state.borrow_mut();
                        *scroll_state = scroll_state.position(list_state.offset());
                    }
                }
                // log::info!("ScrollUp duration: {:?}", start_scroll_up.elapsed());
            },
            MouseEventKind::ScrollLeft => self.report_scroll_state.borrow_mut().scroll_left(),
            MouseEventKind::ScrollRight => self.report_scroll_state.borrow_mut().scroll_right(),
            _ => {
                if !*self.is_popup_open.borrow() {
                    if let Some(scroll_area) = *self.scroll_area.borrow() {
                        let scroll_area_contains_mouse = scroll_area.contains(mouse_position);
                        if scroll_area_contains_mouse {
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

                    // if let Some(scroll_area) = *self.scroll_area.borrow() { // was gonna allow mouse to click and drag for report area
                    //     let scroll_area_contains_mouse = scroll_area.contains(mouse_position);
                    //     if scroll_area_contains_mouse {
                    //         if let MouseEventKind::Drag(MouseButton::Left) = mouse_event.kind {
                    //             let click_row = (r - scroll_area.y) as usize;
                    //             let scroll_area_height = scroll_area.height as usize;
                    //             let total_items = *self.total_items.borrow();
                    //             let visible_height = *self.visible_height.borrow();
                    //             let scrollable_length = total_items.saturating_sub(visible_height);
                    //             let new_offset = (click_row * scrollable_length) / scroll_area_height;
                    //             let mut list_state = self.list_state.borrow_mut();
                    //             let max_offset = scrollable_length;
                    //             *list_state.offset_mut() = new_offset.min(max_offset);
                    //             let mut scroll_state = self.list_scroll_state.borrow_mut();
                    //             *scroll_state = scroll_state.position(list_state.offset());
                    //         }
                    //     }
                    // }

                    match mouse_event.kind {
                        MouseEventKind::Moved => {
                            // Popup hover handling - Check first and take priority
                            if let Some((widget_id, popup_area)) = &*self.active_popup.borrow() {
                                let popup_contains_mouse = popup_area.contains(mouse_position);
                                if popup_contains_mouse {
                                    let content_start_y = popup_area.y + 1; // Top border
                                    let mut popup_state = self.popup_list_state.borrow_mut();
                                    let mut list_state = self.list_state.borrow_mut();
                                    if r >= content_start_y { // Prevent overflow
                                        let relative_row = (r - content_start_y) as usize;
                                        let span_count = self.popup_items.borrow().get(&widget_id.0).map_or(1, |items| items.len());
                                        if relative_row < span_count {
                                            popup_state.select(Some(relative_row));
                                            list_state.select(None); // Deselect checklist
                                        } else {
                                            popup_state.select(None);
                                        }
                                    } else {
                                        popup_state.select(None);
                                    }
                                } else {
                                    let mut popup_state = self.popup_list_state.borrow_mut();
                                    popup_state.select(None);
                                }
                            }
            

                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            let mut popup_clicked = false;
                            let mut button_clicked = false;
                            // Handle clicks within popup
                            if let Some((widget_id, popup_area)) = &*self.active_popup.borrow() {
                                let popup_contains_mouse = popup_area.contains(mouse_position);
        
                                if popup_contains_mouse {
                                    let content_start_y = popup_area.y + 1; // Top border
                                    if r >= content_start_y { // Prevent overflow
                                        let relative_row = (r - content_start_y) as usize;
                                        let mut popup_items = self.popup_items.borrow_mut();
                                        let span_count = popup_items.get(&widget_id.0).map_or(1, |items| items.len());
                                        if relative_row < span_count {
                                            let mut popup_state = self.popup_list_state.borrow_mut();
                                            let mut list_state = self.list_state.borrow_mut();
                                            popup_state.select(Some(relative_row));
                                            list_state.select(None); // Deselect checklist
                                            popup_clicked = true;
                                            // self.handle_popup_action(widget_id, relative_row);
                                            // CHANGED: Toggle status of the TodoItem
                                            if let Some(items) = popup_items.get_mut(&widget_id.0) {
                                                if let Some(item) = items.get_mut(relative_row) {
                                                    item.status = match item.status {
                                                        Status::Todo => Status::Completed,
                                                        Status::Completed => Status::Todo,
                                                    };
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        
                            let buttons = [
                                &self.tuneup_btn,
                                &self.informational_btn,
                                &self.stress_tests_btn,
                            ];
            
                            for button in buttons.iter() {
                                if let Some(btn_area) = button.get_area() {
                                    let btn_contains_mouse = btn_area.contains(mouse_position);
                                    if btn_contains_mouse {
                                        button_clicked = true;
                                        break;
                                    }
                                }
                            }
        
                            // Dismiss popup if clicked outside both popup and buttons
                            if !popup_clicked && !button_clicked {
                                self.active_popup.replace(None);
                                let mut popup_state = self.popup_list_state.borrow_mut();
                                popup_state.select(None);
                            }
        
                            // Checklist click handling - Only if neither popup nor button was clicked
                            if !popup_clicked && !button_clicked {
                                if let Some(checklist_area) = *self.checklist_area.borrow() {
                                    let checklist_contains_mouse = checklist_area.contains(mouse_position);
                                    if checklist_contains_mouse {
                                        let content_start_y = checklist_area.y + 1; // Top border
                                        if r >= content_start_y {
                                            let relative_row = (r - content_start_y) as usize;
                                            let total_items = *self.total_items.borrow();
                                            if relative_row < total_items {
                                                log::info!("Clicked checklist item {} at row {}", relative_row, r);
                                                let mut list_state = self.list_state.borrow_mut();
                                                let mut popup_state = self.popup_list_state.borrow_mut();
                                                list_state.select(Some(relative_row));
                                                popup_state.select(None); // Deselect popup
                                            }
                                        }
                                    }
                                }
                            }
        
                            if self.active_popup.borrow().is_none() {
                                // Checklist click handling
                                if let Some(checklist_area) = *self.checklist_area.borrow() {
                                    let checklist_contains_mouse = checklist_area.contains(mouse_position);
                                    if checklist_contains_mouse {
                                        let content_start_y = checklist_area.y + 1; // Top border
                                        let mut list_state = self.list_state.borrow_mut();
                                        let mut popup_state = self.popup_list_state.borrow_mut();
                                        if r >= content_start_y {
                                            let relative_row = (r - content_start_y) as usize;
                                            let total_items = *self.total_items.borrow();
                                            if relative_row < total_items {
                                                list_state.select(Some(relative_row));
                                                popup_state.select(None); // Deselect popup
                                            }
                                        } else {
                                            list_state.select(None);
                                        }
                                    } else {
                                        let mut list_state = self.list_state.borrow_mut();
                                        let mut popup_state = self.popup_list_state.borrow_mut();
                                        list_state.select(None);
                                        popup_state.select(None); // Ensure popup stays deselected
                                    }
                                }
                            }
                        }
                        MouseEventKind::Down(MouseButton::Right) => {
                            let buttons = [
                                ("Tuneup / QC", &self.tuneup_btn),
                                ("Informational", &self.informational_btn),
                                ("Stress Tests", &self.stress_tests_btn),
                            ];
        
                            for (widget_id, button) in buttons.iter() {
                                if let Some(btn_area) = button.get_area() {
                                    if btn_area.contains(mouse_position) {
                                        let mut popup_items = self.popup_items.borrow_mut();
                                        if let Some(items) = popup_items.get_mut(*widget_id) {
                                            // CHANGED: Check if any item is selected
                                            let any_selected = items.iter().any(|item| item.status == Status::Completed);
                                            let action = if any_selected { "Deselecting" } else { "Selecting" };
                                            log::info!("Right-clicked {} button, {} all items", widget_id, action);
        
                                            for item in items.iter_mut() {
                                                item.status = if any_selected {
                                                    Status::Todo
                                                } else {
                                                    Status::Completed
                                                };
                                                log::info!("{}: {}", action, item.text);
                                            }

                                            /*
                                                or do this if i want to select ONLY unselected items 
                                                when some are already selected instead of deselecting
                                                all of them

                                                for item in items.iter_mut() {
                                                    if item.status == Status::Todo {
                                                        item.status = Status::Completed;
                                                        log::info!("Selected: {}", item.text);
                                                    }
                                                }
                                            */
        
                                            // Open the popup to show the updated selection
                                            if let Some(frame_area) = *self.frame_area.borrow() {
                                                let items = popup_items.get(*widget_id);
                                                let item_count = items.map_or(2, |items| items.len()).max(1);
                                                let popup_height = item_count as u16 + 2;
                                                let popup_width = items
                                                    .map(|items| {
                                                        items.iter()
                                                            .map(|item| item.text.len())
                                                            .max()
                                                            .unwrap_or(10) + 2
                                                    })
                                                    .unwrap_or(12) as u16;
        
                                                let popup_x = btn_area.x + btn_area.width;
                                                let popup_y = btn_area.y;
                                                let adjusted_x = popup_x.min(frame_area.width.saturating_sub(popup_width));
                                                let adjusted_y = popup_y.min(frame_area.height.saturating_sub(popup_height));
                                                let popup_area = Rect::new(adjusted_x, adjusted_y, popup_width, popup_height);
                                                self.active_popup.replace(Some((WidgetId(widget_id.to_string()), popup_area)));
                                                self.list_state.borrow_mut().select(None);
                                                self.popup_list_state.borrow_mut().select(None);
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        },
                        _ => {}
                    }
                    
                    self.tuneup_btn.handle_mouse_event(&mouse_event);
                    self.informational_btn.handle_mouse_event(&mouse_event);
                    self.stress_tests_btn.handle_mouse_event(&mouse_event);
                    self.run_btn.handle_mouse_event(&mouse_event);
                    self.user_scripts_btn.handle_mouse_event(&mouse_event);

                    // Stress-test button: left-click starts a run, right-click
                    // cycles the selected stressor.  We dispatch by hit-testing
                    // against the button's last rendered area instead of going
                    // through the button's own callback (the existing buttons
                    // use ActionHandler, but stress runs need direct calls into
                    // `start_stress_run` / `cycle_stress_choice`).
                    if let Some(btn_area) = self.stress_test_btn.get_area() {
                        if btn_area.contains(mouse_position) {
                            match mouse_event.kind {
                                MouseEventKind::Down(MouseButton::Left) => {
                                    if self.stress_run.borrow().is_some() {
                                        self.stop_stress_run();
                                    } else {
                                        self.start_stress_run();
                                    }
                                }
                                MouseEventKind::Down(MouseButton::Right) => {
                                    self.cycle_stress_choice();
                                }
                                _ => {}
                            }
                        }
                    }
                } else {
                    self.custom_path_field.handle_mouse_event(&mouse_event);
                    for btn in self.data_path_buttons.iter() {
                        btn.handle_mouse_event(&mouse_event);
                    }
                }
            }
        }
        // log::info!("Total handle_mouse_event duration: {:?}", start_total.elapsed());
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        log::info!("KEY EVENT: {key_event:?}");
        match key_event.code {
            KeyCode::Right => {
                log::info!("RIGHT");
                for _ in 0..30 { self.report_scroll_state.borrow_mut().scroll_right(); }
                true
            },
            KeyCode::Left => {
                for _ in 0..30 { self.report_scroll_state.borrow_mut().scroll_left(); }
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
                let mut popup_open = self.is_popup_open.borrow_mut();
                if *popup_open {
                    *popup_open = false;
                } else {
                    let mut list_state = self.list_state.borrow_mut();
                    list_state.select(None);
                }
                true
            }
            KeyCode::Enter => {
                let popup_open_res = self.is_popup_open.try_borrow();
                if let Ok(popup_open) = popup_open_res {
                    if !*popup_open {
                        self.log_message("POPUP NOT OPEN");
                        let list_state = self.list_state.borrow();
                        if let Some(selected) = list_state.selected() {
                            let (full_list, item_to_flat_index) = self.build_full_list();

                            let flat_selected = item_to_flat_index.iter().position(|&i| i == selected).unwrap();
                            let (current_category, current_item) = full_list[flat_selected].clone();
                            log::info!("Current Category: {:?}\nCurrent Item: {:?}", current_category, current_item);
                        }
                    } else {
                        #[cfg(target_os="windows")]
                        {
                            self.log_message("POPUP OPEN");
                            let custom_path = self.custom_path_field.get_text()[0].clone();
                            if !custom_path.is_empty() {
                                self.log_message(format!("CUSTOM PATH: {:?}", custom_path));
                                let tx = self.path_size_tx.clone();
                                std::thread::spawn(move || {
                                    let dir_size = get_directory_size(Path::new(&custom_path));
                                    let _ = tx.try_send(vec![(custom_path, format_size(dir_size))]);
                                });
    
                                if let Ok(mut input) = self.custom_path_field.input.try_borrow_mut() {
                                    input.select_all();
                                    input.cut();
                                }
                            }
                        }
                    }
                }
                true
            }
            KeyCode::PageUp => {
                self.report_scroll_state.borrow_mut().scroll_page_up();
                *self.has_scrolled_manually.borrow_mut() = true;
                true
            }
            KeyCode::PageDown => {
                self.report_scroll_state.borrow_mut().scroll_page_down();
                *self.has_scrolled_manually.borrow_mut() = true;
                true
            }
            _ => {
                let popup_open_res = self.is_popup_open.try_borrow();
                if let Ok(popup_open) = popup_open_res {
                    if !*popup_open {
                        self.service_number_field.handle_key_event(&key_event);
                    } else {
                        self.custom_path_field.handle_key_event(&key_event);
                    }
                }
                false
            }
        }
    }
}

// Checklist hover handling - Only if popup isn’t handling it
// if self.active_popup.borrow().is_none() {
//     if let Some(checklist_area) = *self.checklist_area.borrow() {
//         let checklist_area_contains_mouse = checklist_area.contains(mouse_position);
//         if checklist_area_contains_mouse {
//             let content_start_y = checklist_area.y + 1; // Top border
//             let mut list_state = self.list_state.borrow_mut();
//             let mut popup_state = self.popup_list_state.borrow_mut();
//             if r >= content_start_y {
//                 let relative_row = (r - content_start_y) as usize;
//                 let total_items = *self.total_items.borrow();
//                 if relative_row < total_items {
//                     list_state.select(Some(relative_row));
//                     popup_state.select(None); // Deselect popup
//                 } else {
//                     list_state.select(None);
//                 }
//             } else {
//                 list_state.select(None);
//             }
//         } else {
//             let mut list_state = self.list_state.borrow_mut();
//             let mut popup_state = self.popup_list_state.borrow_mut();
//             list_state.select(None);
//             popup_state.select(None); // Ensure popup stays deselected
//         }
//     }
// }
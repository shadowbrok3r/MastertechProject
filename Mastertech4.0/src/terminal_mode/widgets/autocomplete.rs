use ratatui::{buffer::Buffer, crossterm::event::{KeyCode, KeyModifiers}, layout::{Position, Rect}, style::{Color, Style, Stylize}, text::{Line, Span}, widgets::{Block, BorderType, Borders, Clear, List, ListItem, Widget, WidgetRef}};
use crate::terminal_mode::{events::action_handler::{get_event_sender, WidgetEvent, WidgetId}, styling::{CATPPUCCIN, ThemeRole, THEME}};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use super::{button::{ButtonState, Theme}, ButtonType};
use tui_textarea::{CursorMove, TextArea};
use std::{cell::RefCell, rc::Rc};
use crossbeam::channel::Sender;
use textwrap::refill;

// ---------------------------------------------------------------------------
// AutoCompleteInput: InputField with autocomplete popup.
#[derive(Clone, Debug)]
pub struct AutoCompleteInput<'a> {
    id: WidgetId,
    pub input: RefCell<TextArea<'a>>,
    title: &'static str,
    area: RefCell<Option<Rect>>,
    state: RefCell<ButtonState>,
    theme: ThemeRole,
    block: RefCell<Option<Block<'a>>>,
    event_sender: Sender<WidgetEvent>,
    has_wrapped: Rc<RefCell<bool>>,
    last_width: RefCell<Option<usize>>,
    pub suggestions: RefCell<Vec<String>>,
    max_suggestions: usize,
    highlight: bool,
    selected_index: RefCell<Option<usize>>,
    match_results: RefCell<Vec<(String, i64, Vec<usize>)>>,
    popup_area: RefCell<Option<Rect>>,
    content_start: RefCell<Option<usize>>,
    content_y: RefCell<Option<u16>>,
}

impl<'a> AutoCompleteInput<'a> {
    pub fn new(title: &'static str, id: WidgetId) -> Self {
        let mut text_area = TextArea::default();
        text_area.set_style(Style::default().fg(CATPPUCCIN.text));

        let input = RefCell::new(text_area);

        Self {
            id,
            input,
            title,
            block: RefCell::new(None),
            area: RefCell::new(None),
            state: RefCell::new(ButtonState::Normal),
            theme: ThemeRole::Input,
            event_sender: get_event_sender(),
            has_wrapped: Rc::new(RefCell::new(false)),
            last_width: RefCell::new(None),
            suggestions: RefCell::new(vec![]),
            max_suggestions: 10,
            highlight: false,
            selected_index: RefCell::new(None),
            match_results: RefCell::new(vec![]),
            popup_area: RefCell::new(None),
            content_start: RefCell::new(None),
            content_y: RefCell::new(None),
        }
    }

    pub fn max_suggestions(mut self, max: usize) -> Self {
        self.max_suggestions = max;
        self
    }

    pub fn highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }

    pub fn _id(&self) -> WidgetId {
        self.id.clone()
    }

    fn set_cursor(&self) {
        let mut input = self.input.borrow_mut();
        match *self.state.borrow() {
            ButtonState::Active => input.set_cursor_style(Style::default().fg(Color::Cyan).not_hidden()),
            _ => input.set_cursor_style(Style::default().hidden())
        }
    }

    pub fn _set_block(&self, block: Block<'a>) {
        self.block.replace(Some(block));
    }

    pub fn _get_text(&self) -> Vec<String> {
        self.input.borrow().lines().to_vec()
    }

    pub fn _get_popup_area(&self) -> Option<Rect> {
        if let Ok(popup_area) = self.popup_area.try_borrow() {
            return *popup_area;
        } else {
            return None;
        }
    }

    pub fn get_cursor_position(&self) -> Option<Position> {
        let area = self.get_area()?;
        let input = self.input.borrow();
        let (row, col) = input.cursor();

        let cursor_x = area.x + 1 + col as u16;
        let cursor_y = area.y + 1 + row as u16;

        if cursor_x < area.x + area.width && cursor_y < area.y + area.height {
            Some(Position::new(cursor_x, cursor_y))
        } else {
            None
        }
    }

    pub fn check_text_wrapping(&self, area: Rect) {
        let Ok(mut input) = self.input.try_borrow_mut() else { return; };

        let width = area.width.saturating_sub(2) as usize;
        let lines = input.lines().to_vec();
        let cursor_line_idx = input.cursor().0;
        let cursor_col = input.cursor().1;
        let current_line = lines.get(cursor_line_idx).cloned().unwrap_or(String::new());

        let is_current_clipped = current_line.chars().count() > width && cursor_col >= width;
        let mut has_wrapped = self.has_wrapped.borrow_mut();
        if is_current_clipped && !has_wrapped.clone() {
            input.insert_newline();
            *has_wrapped = true;
        } else if !is_current_clipped {
            *has_wrapped = false;
        }

        let mut last_width = self.last_width.borrow_mut();
        if let Some(prev_width) = *last_width {
            if width > prev_width {
                let current_text = lines.join("\n");
                let refilled = refill(&current_text, width);
                input.select_all();
                input.cut();
                input.insert_str(&refilled);
            }
        }

        *last_width = Some(width);

        let lines = input.lines();

        let mut needs_split = false;
        for (i, line) in lines.iter().enumerate() {
            if line.chars().count() > width {
                needs_split = true;
                input.move_cursor(CursorMove::Jump(i as u16, width as u16));
                input.insert_newline();
                break;
            }
        }
        if needs_split && !is_current_clipped {
            *has_wrapped = false;
        }
    }

    fn accept(&self, index: usize) {
        let Ok(mut input) = self.input.try_borrow_mut() else { return; };
        let match_r = self.match_results.borrow();
        if index >= match_r.len() {
            return;
        }
        let selected = match_r[index].0.clone();
        let (row, col) = input.cursor();
        let current_line = input.lines().get(row).cloned().unwrap_or_default();
        let text_before_cursor = &current_line[0..col.min(current_line.len())];
        let at_byte_pos = text_before_cursor.rfind('@');
        let start_byte = if let Some(pos) = at_byte_pos { pos + 1 } else { 0 };
        let match_text = &text_before_cursor[start_byte..];
        let num_chars_delete = match_text.chars().count();
        input.move_cursor(CursorMove::Jump(row as u16, start_byte as u16));
        for _ in 0..num_chars_delete {
            input.delete_next_char();
        }
        input.insert_str(&selected);
    }

    fn build_highlighted_line(&self, text: &str, match_indices: &[usize]) -> Line<'a> {
        let mut spans: Vec<Span<'a>> = vec![];
        let highlight_color = Color::Rgb(191, 33, 101);
        let normal_style = Style::default().fg(CATPPUCCIN.text);
        let high_style = Style::default().fg(highlight_color);
        let mut current = String::new();
        for (char_idx, c) in text.chars().enumerate() {
            if match_indices.contains(&char_idx) {
                if !current.is_empty() {
                    spans.push(Span::styled(current, normal_style));
                    current = String::new();
                }
                spans.push(Span::styled(c.to_string(), high_style));
            } else {
                current.push(c);
            }
        }
        if !current.is_empty() {
            spans.push(Span::styled(current, normal_style));
        }
        Line::from(spans)
    }

    pub fn render_popup(&self, buf: &mut Buffer, virtual_area: Rect, scroll_offset: Position) {
        let match_r = self.match_results.borrow();
        if !self.is_active() || match_r.is_empty() {
            self.content_start.replace(None);
            self.content_y.replace(None);
            return;
        }
        let popup_bg = THEME.bg;
        let selected_bg = CATPPUCCIN.surface1;
        let num_items = match_r.len().min(self.max_suggestions);
        let max_width = match_r.iter().take(self.max_suggestions).map(|(s, _, _)| s.len()).max().unwrap_or(10) + 4;
        let popup_width = virtual_area.width.max(max_width as u16);
        let popup_height = (num_items + 2) as u16;
        let virtual_popup_x = virtual_area.x;
        let virtual_popup_y = virtual_area.y + (virtual_area.height * 2);
        let physical_popup_x = virtual_popup_x.saturating_sub(scroll_offset.x);
        let physical_popup_y = virtual_popup_y.saturating_sub(scroll_offset.y);
        let clip_top_pixels = scroll_offset.y.saturating_sub(virtual_popup_y) as usize;
        let original_effective = popup_height as usize - clip_top_pixels;
        let visible_area = buf.area();
        let max_y = visible_area.y + visible_area.height;
        let max_height = (max_y.saturating_sub(physical_popup_y)) as usize;
        let clipped_bottom = original_effective > max_height;
        let has_bottom = !clipped_bottom;
        let effective_height = original_effective.min(max_height);
        if effective_height < 2 {
            self.content_start.replace(None);
            self.content_y.replace(None);
            return;
        }
        let has_top = clip_top_pixels == 0;
        let mut borders = Borders::LEFT | Borders::RIGHT;
        if has_top {
            borders |= Borders::TOP;
        }
        if has_bottom {
            borders |= Borders::BOTTOM;
        }
        let content_height = effective_height - if has_top { 1 } else { 0 } - if has_bottom { 1 } else { 0 };
        if content_height == 0 {
            self.content_start.replace(None);
            self.content_y.replace(None);
            return;
        }
        let content_start_index = if clip_top_pixels > 0 { clip_top_pixels - 1 } else { 0 };
        let lines_len = match_r.len().min(self.max_suggestions);
        if content_start_index >= lines_len {
            return;
        }
        let content_end_index = content_start_index + content_height;
        let content_height = content_end_index.min(lines_len) - content_start_index;
        let effective_height = content_height + if has_top { 1 } else { 0 } + if has_bottom { 1 } else { 0 };
        let physical_popup_rect = Rect::new(physical_popup_x, physical_popup_y, popup_width, effective_height as u16);
        let block = Block::default()
            .borders(borders)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(THEME.accent))
            .style(Style::new().bg(popup_bg).fg(CATPPUCCIN.sky));
        Clear.render(physical_popup_rect, buf);
        buf.set_style(physical_popup_rect, Style::default().bg(popup_bg));
        let inner_area = block.inner(physical_popup_rect);
        block.render(physical_popup_rect, buf);
        let mut list_items: Vec<ListItem> = vec![];
        for (i, (s, _, indices)) in match_r.iter().skip(content_start_index).take(content_height).enumerate() {
            let abs_i = content_start_index + i;
            let line = if self.highlight {
                self.build_highlighted_line(s, indices)
            } else {
                Line::raw(s.clone())
            };
            let item_style = if Some(abs_i) == *self.selected_index.borrow() {
                Style::default().bg(selected_bg)
            } else {
                Style::default().bg(popup_bg)
            };
            let item = ListItem::new(line).style(item_style);
            list_items.push(item);
        }
        let list = List::new(list_items);
        list.render(inner_area, buf);
        self.popup_area.replace(Some(physical_popup_rect));
        self.content_start.replace(Some(content_start_index));
        self.content_y.replace(Some(inner_area.y));
    }
}

impl<'a> ButtonType<'a> for AutoCompleteInput<'a> {
    fn set_state(&self, state: ButtonState) {
        self.state.replace(state);
        self.set_cursor();
    }

    fn get_area(&self) -> Option<Rect> {
        *self.area.borrow()
    }

    fn set_area(&self, area: Rect) {
        self.area.replace(Some(area));
    }

    fn is_active(&self) -> bool {
        matches!(*self.state.borrow(), ButtonState::Active)
    }

    fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme.resolve();
        match *self.state.borrow() {
            ButtonState::Normal => (CATPPUCCIN.lavender, t.text, t.shadow, t.highlight),
            ButtonState::Selected => (CATPPUCCIN.blue, CATPPUCCIN.text, CATPPUCCIN.sapphire, CATPPUCCIN.red),
            ButtonState::Active => (CATPPUCCIN.green, CATPPUCCIN.teal, CATPPUCCIN.red, CATPPUCCIN.blue),
            ButtonState::Hovered => (CATPPUCCIN.lavender, CATPPUCCIN.sapphire, CATPPUCCIN.red, CATPPUCCIN.maroon),
            ButtonState::AltClicked => (CATPPUCCIN.maroon, CATPPUCCIN.maroon, CATPPUCCIN.maroon, CATPPUCCIN.maroon),
        }
    }

    fn handle_key_event(&self, key_event: &ratatui::crossterm::event::KeyEvent) -> bool {
        let popup_visible = !self.match_results.borrow().is_empty();
        if popup_visible {
            match key_event.code {
                KeyCode::Up => {
                    let mut idx = self.selected_index.borrow_mut();
                    if let Some(i) = *idx {
                        if i > 0 {
                            *idx = Some(i - 1);
                        }
                    } else {
                        *idx = Some(0);
                    }
                    return true;
                }
                KeyCode::Down => {
                    let max = self.match_results.borrow().len().min(self.max_suggestions) - 1;
                    let mut idx = self.selected_index.borrow_mut();
                    if let Some(i) = *idx {
                        if i < max {
                            *idx = Some(i + 1);
                        }
                    } else {
                        *idx = Some(0);
                    }
                    return true;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    if let Some(i) = *self.selected_index.borrow() {
                        self.accept(i);
                    }
                    self.match_results.replace(vec![]);
                    self.selected_index.replace(None);
                    return true;
                }
                KeyCode::Esc => {
                    self.match_results.replace(vec![]);
                    self.selected_index.replace(None);
                    return true;
                }
                _ => {}
            }
        }
        let mut input = self.input.borrow_mut();
        let modifiers = key_event.modifiers;
        let handled = match key_event.code {
            KeyCode::Backspace => {
                let (row, col) = input.cursor();
                if col == 0 && row > 0 {
                    input.move_cursor(CursorMove::Up);
                    input.move_cursor(CursorMove::End);
                    input.delete_char();
                    true
                } else {
                    input.input_without_shortcuts(*key_event)
                }
            }
            KeyCode::End => {
                input.move_cursor(CursorMove::End);
                true
            }
            KeyCode::Home => {
                input.move_cursor(CursorMove::Head);
                true
            }
            KeyCode::Up => {
                input.move_cursor(CursorMove::Up);
                true
            }
            KeyCode::Down => {
                input.move_cursor(CursorMove::Down);
                true
            }
            KeyCode::Left => {
                input.move_cursor(CursorMove::Back);
                true
            }
            KeyCode::Right => {
                input.move_cursor(CursorMove::Forward);
                true
            }
            KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => {
                input.select_all();
                true
            }
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                input.select_all();
                input.copy();
                let mut clipboard = arboard::Clipboard::new().unwrap();
                let set_clipboard_contents = clipboard.set().text(input.yank_text());
                log::info!("Set clipboard contents: {set_clipboard_contents:?}");
                true
            }
            KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
                let mut clipboard = arboard::Clipboard::new().unwrap();
                let get_clipboard_contents = clipboard.get().text();
                log::error!("CLIP CONTENTS: {get_clipboard_contents:?}");
                if let Ok(contents) = get_clipboard_contents {
                    input.insert_str(contents);
                    // self.insert_wrapped_text(contents, area.width.saturating_sub(2) as usize);
                }
                true
            }
            KeyCode::Char(_) if modifiers.contains(KeyModifiers::SHIFT) => input.input(*key_event),
            _ => {
                input.input_without_shortcuts(*key_event);
                log::error!("updating matches");
                let (row, col) = input.cursor();
                let current_line = input.lines().get(row).cloned().unwrap_or_default();
                let text_before_cursor = &current_line[0..col.min(current_line.len())];
                let match_text = text_before_cursor;
                if match_text.chars().count() <= 1 {
                    self.match_results.replace(vec![]);
                    self.selected_index.replace(None);
                    // return;
                }
                let matcher = SkimMatcherV2::default().ignore_case();
                let mut results: Vec<(String, i64, Vec<usize>)> = self
                    .suggestions
                    .borrow()
                    .iter()
                    .filter_map(|s| matcher.fuzzy_indices(s, match_text).map(|(score, indices)| (s.clone(), score, indices)))
                    .collect();
                results.sort_by_key(|(_, score, _)| std::cmp::Reverse(*score));
                self.match_results.replace(results);
                self.selected_index.replace(None);
                true
            },
        };
        
        // self.update_matches();

        handled
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let popup_opt = *self.popup_area.borrow();
        let content_start_opt = *self.content_start.borrow();
        let content_y_opt = *self.content_y.borrow();
        if let Some(popup_area) = popup_opt {
            let pos = Position::new(mouse_event.column, mouse_event.row);
            let inside_popup = popup_area.contains(pos);
            match mouse_event.kind {
                MouseEventKind::Moved => {
                    if inside_popup {
                        if let (Some(content_start), Some(content_y)) = (content_start_opt, content_y_opt) {
                            log::info!("\nmouse pos: {pos:?}\ncontent_start: {content_start:?}\ncontent_y: {content_y:?}\npopup_area: {popup_area:?}");
                            if mouse_event.row >= content_y {
                                let rel_row = (mouse_event.row - content_y) as usize + content_start;
                                if rel_row < self.match_results.borrow().len().min(self.max_suggestions) {
                                    self.selected_index.replace(Some(rel_row));
                                }
                            }
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if inside_popup {
                        if let (Some(content_start), Some(content_y)) = (content_start_opt, content_y_opt) {
                            if mouse_event.row >= content_y {
                                let rel_row = (mouse_event.row - content_y) as usize + content_start;
                                if rel_row < self.match_results.borrow().len().min(self.max_suggestions) {
                                    self.accept(rel_row);
                                    self.match_results.replace(vec![]);
                                    self.selected_index.replace(None);
                                }
                            }
                        }
                        return;
                    }
                }
                _ => {}
            }
        }
        let Some(area) = *self.area.borrow() else { return; };
        let inside = area.contains(Position::new(mouse_event.column, mouse_event.row));
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if inside {
                    if self.is_active() {
                        self.set_state(ButtonState::Normal);
                    } else {
                        self.set_state(ButtonState::Active);
                        let _ = self.event_sender.try_send(WidgetEvent::Active { widget_id: self.id.clone() });
                    }
                } else {
                    self.set_state(ButtonState::Normal);
                    self.match_results.replace(vec![]);
                    self.selected_index.replace(None);
                }
            }
            MouseEventKind::Moved => {
                if !self.is_active() {
                    if inside {
                        self.set_state(ButtonState::Hovered);
                    } else {
                        self.set_state(ButtonState::Normal);
                    }
                }
            }
            _ => {}
        }
    }
}

impl<'a> WidgetRef for AutoCompleteInput<'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (_background, text_color, _shadow, highlight) = self.colors();
        self.set_area(area);
        self.check_text_wrapping(area);
        let default_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::raw(self.title).fg(text_color))
            .style(Style::default().fg(highlight));
        let input_try = self.input.try_borrow_mut();
        if let Ok(mut input) = input_try {
            let block = self.block.borrow().clone().unwrap_or(default_block);
            input.set_block(block);
            input.render(area, buf);
        }
    }
}
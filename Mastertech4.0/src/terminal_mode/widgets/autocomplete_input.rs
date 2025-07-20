use ratatui::{
    buffer::Buffer, 
    crossterm::event::{KeyCode, KeyModifiers}, 
    layout::{Position, Rect}, 
    style::{Color, Style, Stylize}, 
    text::Line, 
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Widget, WidgetRef}
};
use crate::terminal_mode::{
    events::action_handler::{get_event_sender, WidgetEvent, WidgetId}, 
    styling::{CATPPUCCIN, CATPPUCCINTHEME}
};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use super::{button::{ButtonState, Theme}, ButtonType};
use crossbeam::channel::Sender;
use tui_textarea::{CursorMove, TextArea};
use std::{cell::RefCell, rc::Rc};

/// AutoCompleteInput: An input field with popup autocomplete suggestions
#[derive(Clone, Debug)]
pub struct AutoCompleteInput<'a> {
    id: WidgetId,
    /// The underlying tui‑input state.
    pub input: RefCell<TextArea<'a>>,
    /// Title shown as the field's label.
    title: &'static str,
    /// Store the last drawn area.
    area: RefCell<Option<Rect>>,
    /// Store the popup area for mouse interaction
    popup_area: RefCell<Option<Rect>>,
    /// state of input field
    state: RefCell<ButtonState>,
    /// duh
    theme: Theme,
    block: RefCell<Option<Block<'a>>>,
    event_sender: Sender<WidgetEvent>,
    has_wrapped: Rc<RefCell<bool>>,
    last_width: RefCell<Option<usize>>,
    /// Autocomplete suggestions
    suggestions: RefCell<Vec<String>>,
    /// Show popup flag
    show_popup: RefCell<bool>,
    /// Selected suggestion index
    selected_suggestion: RefCell<Option<usize>>,
    /// Track area and offset for mouse coordinate adjustment
    total_offset: RefCell<u16>,
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
            popup_area: RefCell::new(None),
            state: RefCell::new(ButtonState::Normal),
            theme: CATPPUCCINTHEME,
            event_sender: get_event_sender(),
            has_wrapped: Rc::new(RefCell::new(false)),
            last_width: RefCell::new(None),
            suggestions: RefCell::new(vec![
                "John Smith".to_string(),
                "Jane Doe".to_string(),
                "Bob Johnson".to_string(),
                "Alice Brown".to_string(),
                "Charlie Wilson".to_string(),
                "Diana Clark".to_string(),
                "Eve Adams".to_string(),
                "Frank Miller".to_string(),
            ]),
            show_popup: RefCell::new(false),
            selected_suggestion: RefCell::new(None),
            total_offset: RefCell::new(0),
        }
    }

    pub fn set_total_offset(&self, offset: u16) {
        self.total_offset.replace(offset);
    }

    pub fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn set_cursor(&self) {
        let mut input = self.input.borrow_mut();
        match *self.state.borrow() {
            ButtonState::Active => input.set_cursor_style(Style::default().fg(Color::Cyan).not_hidden()),
            _ => input.set_cursor_style(Style::default().hidden())
        }
    }

    pub fn set_block(&self, block: Block<'a>) {
        self.block.replace(Some(block));
    }

    pub fn get_text(&self) -> Vec<String> {
        self.input.borrow().lines().to_vec()
    }

    pub fn get_cursor_position(&self) -> Option<Position> {
        let area = self.get_area()?;
        let input = self.input.borrow();
        let (row, col) = input.cursor();

        // Adjust for area's position and borders
        let cursor_x = area.x + 1 + col as u16; // +1 for left border
        let cursor_y = area.y + 1 + row as u16; // +1 for top border

        // Ensure cursor stays within area bounds
        if cursor_x < area.x + area.width && cursor_y < area.y + area.height {
            Some(Position::new(cursor_x, cursor_y))
        } else {
            None
        }
    }

    /// Filter suggestions based on current input
    fn filter_suggestions(&self) -> Vec<String> {
        let input_text = self.input.borrow().lines().get(0).cloned().unwrap_or_default();
        if input_text.is_empty() {
            return Vec::new();
        }

        let suggestions = self.suggestions.borrow();
        suggestions
            .iter()
            .filter(|s| s.to_lowercase().contains(&input_text.to_lowercase()))
            .cloned()
            .collect()
    }

    /// Update popup visibility based on input
    fn update_popup(&self) {
        let filtered = self.filter_suggestions();
        let should_show = !filtered.is_empty() && self.is_active();
        self.show_popup.replace(should_show);
        
        if !should_show {
            self.selected_suggestion.replace(None);
        }
    }

    /// Render the popup if it should be visible
    fn render_popup(&self, buf: &mut Buffer) {
        if !*self.show_popup.borrow() {
            return;
        }

        let Some(area) = *self.area.borrow() else { return; };
        let filtered = self.filter_suggestions();
        if filtered.is_empty() {
            return;
        }

        // Position popup below the input field
        let popup_height = (filtered.len() as u16).min(5) + 2; // +2 for borders
        let popup_area = Rect {
            x: area.x,
            y: area.y + area.height,
            width: area.width,
            height: popup_height,
        };

        // Store popup area for mouse interaction
        self.popup_area.replace(Some(popup_area));

        // Create list items
        let items: Vec<ListItem> = filtered
            .iter()
            .enumerate()
            .map(|(i, suggestion)| {
                let selected = Some(i) == *self.selected_suggestion.borrow();
                let style = if selected {
                    Style::default().bg(CATPPUCCIN.blue).fg(CATPPUCCIN.base)
                } else {
                    Style::default().fg(CATPPUCCIN.text)
                };
                ListItem::new(suggestion.clone()).style(style)
            })
            .collect();

        // Create and render the list
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(CATPPUCCIN.blue))
                    .title("Suggestions")
            )
            .style(Style::default().bg(CATPPUCCIN.mantle));

        let mut list_state = ListState::default();
        if let Some(selected) = *self.selected_suggestion.borrow() {
            list_state.select(Some(selected));
        }

        Widget::render(list, popup_area, buf);
    }

    /// Handle autocomplete-specific key events
    fn handle_autocomplete_keys(&self, key_event: &ratatui::crossterm::event::KeyEvent) -> bool {
        if !*self.show_popup.borrow() {
            return false;
        }

        let filtered = self.filter_suggestions();
        if filtered.is_empty() {
            return false;
        }

        match key_event.code {
            KeyCode::Down => {
                let current = self.selected_suggestion.borrow().unwrap_or(0);
                let new_selection = if current + 1 < filtered.len() {
                    current + 1
                } else {
                    0
                };
                self.selected_suggestion.replace(Some(new_selection));
                true
            }
            KeyCode::Up => {
                let current = self.selected_suggestion.borrow().unwrap_or(0);
                let new_selection = if current == 0 {
                    filtered.len() - 1
                } else {
                    current - 1
                };
                self.selected_suggestion.replace(Some(new_selection));
                true
            }
            KeyCode::Enter | KeyCode::Tab => {
                if let Some(selected_idx) = *self.selected_suggestion.borrow() {
                    if let Some(suggestion) = filtered.get(selected_idx) {
                        // Replace current input with suggestion
                        let mut input = self.input.borrow_mut();
                        input.delete_line_by_head();
                        input.insert_str(suggestion);
                        
                        // Hide popup
                        self.show_popup.replace(false);
                        self.selected_suggestion.replace(None);
                        return true;
                    }
                }
                false
            }
            KeyCode::Esc => {
                self.show_popup.replace(false);
                self.selected_suggestion.replace(None);
                true
            }
            _ => false,
        }
    }

    /// Handle mouse interaction with popup
    fn handle_popup_mouse(&self, mouse_event: &MouseEvent) -> bool {
        if !*self.show_popup.borrow() {
            return false;
        }

        let Some(popup_area) = *self.popup_area.borrow() else { return false; };
        let filtered = self.filter_suggestions();
        if filtered.is_empty() {
            return false;
        }

        let c = mouse_event.column;
        let r = mouse_event.row;

        // Check if mouse is inside popup area (using adjusted coordinates that are already passed in)
        let inside_popup = c >= popup_area.x 
            && c < popup_area.x + popup_area.width 
            && r >= popup_area.y 
            && r < popup_area.y + popup_area.height;

        if !inside_popup {
            return false;
        }

        // Calculate which suggestion item was clicked/hovered (accounting for borders)
        let relative_y = r.saturating_sub(popup_area.y + 1); // +1 for top border
        let suggestion_index = relative_y as usize;

        match mouse_event.kind {
            MouseEventKind::Moved => {
                if suggestion_index < filtered.len() {
                    self.selected_suggestion.replace(Some(suggestion_index));
                }
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if suggestion_index < filtered.len() {
                    if let Some(suggestion) = filtered.get(suggestion_index) {
                        // Replace current input with suggestion
                        let mut input = self.input.borrow_mut();
                        input.delete_line_by_head();
                        input.insert_str(suggestion);
                        
                        // Hide popup
                        self.show_popup.replace(false);
                        self.selected_suggestion.replace(None);
                    }
                }
                true
            }
            _ => false,
        }
    }
}

impl<'a> ButtonType<'a> for AutoCompleteInput<'a> {
    fn set_state(&self, state: ButtonState) {
        self.state.replace(state);
        self.set_cursor();
        
        // Update popup when state changes
        self.update_popup();
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

    /// Helper method to get the right colors based on the current state.
    fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme;
        match *self.state.borrow() {
            ButtonState::Normal => (CATPPUCCIN.lavender, t.text, t.shadow, t.highlight),
            ButtonState::Selected => (CATPPUCCIN.blue, CATPPUCCIN.text, CATPPUCCIN.sapphire, CATPPUCCIN.red),
            ButtonState::Active => (CATPPUCCIN.green, CATPPUCCIN.teal, CATPPUCCIN.red, CATPPUCCIN.blue),
            ButtonState::Hovered => (CATPPUCCIN.lavender, CATPPUCCIN.sapphire, CATPPUCCIN.red, CATPPUCCIN.maroon),
            ButtonState::AltClicked => (CATPPUCCIN.maroon, CATPPUCCIN.maroon, CATPPUCCIN.maroon, CATPPUCCIN.maroon),
        }
    }

    fn handle_key_event(&self, key_event: &ratatui::crossterm::event::KeyEvent) -> bool {
        // First try autocomplete-specific keys
        if self.handle_autocomplete_keys(key_event) {
            return true;
        }

        // Handle regular input
        let mut input = self.input.borrow_mut();
        let modifiers = key_event.modifiers;
        let result = match key_event.code {
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
            KeyCode::Up if !*self.show_popup.borrow() => {
                input.move_cursor(CursorMove::Up);
                true
            }
            KeyCode::Down if !*self.show_popup.borrow() => {
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
                input.copy();
                true
            }
            KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    if let Ok(contents) = clipboard.get_text() {
                        input.insert_str(&contents);
                    }
                }
                true
            }
            _ => input.input_without_shortcuts(*key_event),
        };

        // Update popup after input changes
        drop(input); // Release the borrow
        self.update_popup();
        
        result
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // First check if it's a popup interaction
        if self.handle_popup_mouse(mouse_event) {
            return;
        }

        // Handle regular input field mouse events
        let Some(area) = *self.area.borrow() else { return; };

        let c = mouse_event.column;
        let r = mouse_event.row;

        let inside = c >= area.x 
            && c < area.x + area.width 
            && r >= area.y 
            && r < area.y + area.height;

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
                    // Click outside closes popup and deactivates field
                    self.show_popup.replace(false);
                    self.selected_suggestion.replace(None);
                    self.set_state(ButtonState::Normal);
                }
            }
            MouseEventKind::Moved => {
                // Only change state on hover if we're not already active.
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
        
        // Save the area for later use.
        self.set_area(area);

        // Draw a bordered block with the field's title.
        let default_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::raw(self.title).fg(text_color))
            .style(Style::default().fg(highlight));

        let input = self.input.try_borrow_mut();

        if let Ok(mut input) = input {
            let block = if let Some(block) = self.block.borrow().clone() {
                block
            } else { 
                default_block 
            };
            input.set_block(block);
            input.render(area, buf);
        }

        // Render popup after the main input
        self.render_popup(buf);
    }
}

// Include validation tests
#[cfg(test)]
#[path = "autocomplete_input_test.rs"]
mod tests;
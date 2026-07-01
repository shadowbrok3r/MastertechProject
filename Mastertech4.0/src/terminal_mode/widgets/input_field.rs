use ratatui::{buffer::Buffer, crossterm::event::{KeyCode, KeyModifiers}, layout::{Position, Rect}, style::{Color, Style, Stylize}, text::Line, widgets::{Block, BorderType, Borders, Widget, WidgetRef}};
use crate::terminal_mode::{events::action_handler::{get_event_sender, WidgetEvent, WidgetId}, styling::{CATPPUCCIN, ThemeRole, THEME}};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use super::{button::{ButtonState, Theme}, ButtonType};
use super::tui_textarea::{CursorMove, TextArea};
use std::cell::RefCell;
use std::time::Instant;
use crossbeam::channel::Sender;
use unicode_width::UnicodeWidthChar;

/// Time threshold for detecting double-click (in milliseconds)
const DOUBLE_CLICK_THRESHOLD_MS: u128 = 400;

/// Minimum height required for an InputField to be visible (borders + 1 line of text)
pub const INPUT_FIELD_MIN_HEIGHT: u16 = 3;

// ---------------------------------------------------------------------------
// InputField: A wrapper around tui_input::Input for our form fields.
#[derive(Clone)]
pub struct InputField <'a> {
    id: WidgetId,
    /// Last time a mouse click occurred (for double-click detection)
    last_click_time: RefCell<Option<Instant>>,
    /// Last click position for double-click detection
    last_click_pos: RefCell<Option<(u16, u16)>>,
    /// The underlying tui‑input state.
    pub input: RefCell<TextArea<'a>>,
    /// Title shown as the field’s label.
    title: &'static str,
    /// Store the last drawn area.
    area: RefCell<Option<Rect>>,
    /// state of input field
    state: RefCell<ButtonState>,
    /// duh
    theme: ThemeRole,
    block: RefCell<Option<Block<'a>>>,
    event_sender: Sender<WidgetEvent>,
    last_width: RefCell<Option<usize>>,
}

impl<'a> std::fmt::Debug for InputField<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputField")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("state", &self.state)
            .finish()
    }
}

impl <'a> InputField <'a>{
    pub fn new(title: &'static str, id: WidgetId) -> Self {
        let mut text_area = TextArea::default();
        text_area.set_style(Style::default().fg(CATPPUCCIN.text));
        // Set selection style to match Catppuccin theme - use a visible selection background
        text_area.set_selection_style(Style::default().bg(CATPPUCCIN.surface1).fg(CATPPUCCIN.text));

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
            last_width: RefCell::new(None),
            last_click_time: RefCell::new(None),
            last_click_pos: RefCell::new(None),
        }
    }

    pub fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn set_cursor(&self) {
        let mut input = self.input.borrow_mut();
        match *self.state.borrow() {
            ButtonState::Active | ButtonState::Selecting => {
                input.set_cursor_style(Style::default().fg(THEME.tertiary).not_hidden())
            },
            _ => input.set_cursor_style(Style::default().hidden())
        }
    }

    pub fn set_block(&self, block: Block<'a>) {
        self.block.replace(Some(block));
    }

    pub fn get_text(&self) -> Vec<String> {
        self.input.borrow().lines().to_vec()
    }

    // pub fn insert_wrapped_text(&self, text: String, width: usize) {
    //     let input_result = self.input.try_borrow_mut();
    //     if let Ok(mut input) = input_result {
    //         let wrapped = textwrap::fill(&text, width);

    //         input.delete_line_by_head();
    //         input.insert_str(&wrapped);
    //     }
    // }

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

    pub fn check_text_wrapping(&self, area: Rect) {
        // This method now only tracks width changes without modifying text content.
        // Text wrapping is handled visually by the TextArea widget itself.
        // We no longer insert hard line breaks which caused corruption on resize.
        let Ok(input) = self.input.try_borrow() else { return; };
        
        let width = area.width.saturating_sub(2) as usize;
        let mut last_width = self.last_width.borrow_mut();
        *last_width = Some(width);
        
        // Just track that we've processed this width - no text modification
        drop(input);
    }
    
    /// Get the raw text content as a single string (joining all lines)
    pub fn get_raw_text(&self) -> String {
        let input = self.input.borrow();
        input.lines().iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<&str>>()
            .join(" ")
    }
    
    /// Set text content, properly handling multi-line text
    pub fn set_text(&self, text: &str) {
        if let Ok(mut input) = self.input.try_borrow_mut() {
            input.select_all();
            input.cut();
            // Insert the text as-is - let the TextArea handle wrapping visually
            input.insert_str(text);
        }
    }
    
    /// Convert mouse coordinates (column, row) to text position (row, col) within the InputField.
    /// Returns None if the coordinates are outside the text area.
    fn mouse_to_text_position(&self, mouse_col: u16, mouse_row: u16) -> Option<(usize, usize)> {
        let area = self.get_area()?;
        let input = self.input.try_borrow().ok()?;
        
        // Account for borders (1 pixel on each side)
        let inner_x = area.x + 1;
        let inner_y = area.y + 1;
        let inner_width = area.width.saturating_sub(2);
        let inner_height = area.height.saturating_sub(2);
        
        // Check if mouse is within the inner text area
        if mouse_col < inner_x || mouse_col >= inner_x + inner_width {
            return None;
        }
        if mouse_row < inner_y || mouse_row >= inner_y + inner_height {
            return None;
        }
        
        // Calculate text row based on mouse position and viewport offset
        let viewport_row = input.viewport().0 as usize; // row offset from viewport
        let relative_row = (mouse_row - inner_y) as usize;
        let text_row = viewport_row + relative_row;
        
        // Clamp to valid row range
        let lines = input.lines();
        let text_row = text_row.min(lines.len().saturating_sub(1));
        
        // Calculate text column based on mouse position relative to line content
        let relative_col = (mouse_col - inner_x) as usize;
        let viewport_col = input.viewport().1 as usize; // col offset from viewport
        
        // Get the line at the target row
        let line = lines.get(text_row).map(|s| s.as_str()).unwrap_or("");
        
        // Find the character position that corresponds to the display column
        // We need to account for character widths (especially for wide characters)
        let mut display_col = 0usize;
        let mut char_col = 0usize;
        
        for ch in line.chars().skip(viewport_col) {
            let ch_width = ch.width().unwrap_or(1);
            if display_col + ch_width > relative_col {
                // We've found or passed the target column
                // If we're closer to the next character, round up
                if relative_col > display_col + ch_width / 2 {
                    char_col += 1;
                }
                break;
            }
            display_col += ch_width;
            char_col += 1;
        }
        
        // Add back the viewport column offset and clamp to line length
        let final_col = (viewport_col + char_col).min(line.chars().count());
        
        Some((text_row, final_col))
    }
    
    /// Set selection style for the InputField
    pub fn set_selection_style(&self, style: Style) {
        if let Ok(mut input) = self.input.try_borrow_mut() {
            input.set_selection_style(style);
        }
    }
    
    /// Get the current selection range if any text is selected
    pub fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        self.input.try_borrow().ok()?.selection_range()
    }
    
    /// Check if there's an active text selection
    pub fn has_selection(&self) -> bool {
        self.input.try_borrow().ok().map(|i| i.is_selecting()).unwrap_or(false)
    }
}

impl <'a> ButtonType <'a> for InputField <'a> {
    fn set_state(&self, state: ButtonState) {
        // log::info!("set_state => Setting {:?} to {state:?}", self.title);
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
        matches!(*self.state.borrow(), ButtonState::Active | ButtonState::Selecting)
    }

    /// Helper method to get the right colors based on the current state.
    fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme.resolve();
        match *self.state.borrow() {
            ButtonState::Normal => (t.background, THEME.text_muted, t.shadow, THEME.border_idle()),
            ButtonState::Selected => (t.background, THEME.text, t.shadow, THEME.tertiary),
            ButtonState::Active => (t.background, THEME.text, t.shadow, THEME.accent),
            ButtonState::Hovered => (t.background, THEME.text, t.shadow, THEME.tertiary),
            ButtonState::AltClicked => (t.background, THEME.text_muted, t.shadow, THEME.border_idle()),
            ButtonState::Selecting => (t.background, THEME.text, t.shadow, THEME.accent),
        }
    }

    fn handle_key_event(&self, key_event: &ratatui::crossterm::event::KeyEvent) -> bool {
        let mut input = self.input.borrow_mut();
        let modifiers = key_event.modifiers;
        match key_event.code {
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
            KeyCode::End => { // | KeyCode::Right if modifiers.contains(KeyModifiers::CONTROL)
                input.move_cursor(CursorMove::End);
                true
            }
            KeyCode::Home => { // | KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL)
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
                // Copy selected text to clipboard (or select all if no selection)
                if input.selection_range().is_none() {
                    input.select_all();
                }
                input.copy();
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let yank_text = input.yank_text();
                    if !yank_text.is_empty() {
                        let _ = clipboard.set().text(&yank_text);
                        log::info!("Copied to clipboard: {} chars", yank_text.len());
                    }
                }
                true
            }
            KeyCode::Char('x') if modifiers.contains(KeyModifiers::CONTROL) => {
                // Cut selected text to clipboard (or select all if no selection)
                if input.selection_range().is_none() {
                    input.select_all();
                }
                input.cut();
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let yank_text = input.yank_text();
                    if !yank_text.is_empty() {
                        let _ = clipboard.set().text(&yank_text);
                        log::info!("Cut to clipboard: {} chars", yank_text.len());
                    }
                }
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
            _ => input.input_without_shortcuts(*key_event),
        }
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // If we haven’t assigned an area yet, do nothing
        let Some(area) = *self.area.borrow() else { return; };

        let c = mouse_event.column;
        let r = mouse_event.row;

        let inside = c >= area.x 
            && c < area.x + area.width 
            && r >= area.y 
            && r < area.y + area.height;

        let current_state = *self.state.borrow();
        
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if inside {
                    let now = Instant::now();
                    let last_time = self.last_click_time.borrow().clone();
                    let last_pos = self.last_click_pos.borrow().clone();
                    
                    // Check for double-click: same position within threshold time
                    let is_double_click = if let (Some(last_t), Some((last_c, last_r))) = (last_time, last_pos) {
                        let elapsed = now.duration_since(last_t).as_millis();
                        // Allow small positional tolerance (within 2 characters)
                        let pos_match = (c as i16 - last_c as i16).abs() <= 2 
                            && (r as i16 - last_r as i16).abs() <= 1;
                        elapsed < DOUBLE_CLICK_THRESHOLD_MS && pos_match
                    } else {
                        false
                    };
                    
                    // Update click tracking
                    self.last_click_time.replace(Some(now));
                    self.last_click_pos.replace(Some((c, r)));
                    
                    if is_double_click {
                        // Double-click: select the word under cursor
                        if let Ok(mut input) = self.input.try_borrow_mut() {
                            // First, move cursor to clicked position
                            if let Some((text_row, text_col)) = self.mouse_to_text_position(c, r) {
                                input.move_cursor(CursorMove::Jump(text_row as u16, text_col as u16));
                            }
                            // Then select the word: move to word start, start selection, move to word end
                            input.move_cursor(CursorMove::WordBack);
                            input.start_selection();
                            input.move_cursor(CursorMove::WordForward);
                        }
                        // Reset click tracking to prevent triple-click issues
                        self.last_click_time.replace(None);
                        self.set_state(ButtonState::Active);
                    } else {
                        // Single click: Start potential text selection
                        if let Some((text_row, text_col)) = self.mouse_to_text_position(c, r) {
                            if let Ok(mut input) = self.input.try_borrow_mut() {
                                // Cancel any existing selection first
                                input.cancel_selection();
                                // Move cursor to clicked position
                                input.move_cursor(CursorMove::Jump(text_row as u16, text_col as u16));
                                // Start selection from this position
                                input.start_selection();
                            }
                        }
                        // Set state to Selecting (active with selection in progress)
                        self.set_state(ButtonState::Selecting);
                    }
                    let _ = self.event_sender.try_send(WidgetEvent::Active { widget_id: self.id.clone() });
                } else {
                    // Clicked outside - cancel any selection and deactivate
                    if let Ok(mut input) = self.input.try_borrow_mut() {
                        input.cancel_selection();
                    }
                    self.set_state(ButtonState::Normal);
                    // Reset click tracking when clicking outside
                    self.last_click_time.replace(None);
                    self.last_click_pos.replace(None);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                // Extend selection while dragging (only if we're in selecting state)
                if current_state == ButtonState::Selecting {
                    if let Some((text_row, text_col)) = self.mouse_to_text_position(c, r) {
                        if let Ok(mut input) = self.input.try_borrow_mut() {
                            // Move cursor to extend selection (selection is maintained via start_selection)
                            input.move_cursor(CursorMove::Jump(text_row as u16, text_col as u16));
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // Finish selection
                if current_state == ButtonState::Selecting {
                    // Check if we actually selected any text (different start/end positions)
                    let has_real_selection = if let Ok(input) = self.input.try_borrow() {
                        input.selection_range().map(|(start, end)| start != end).unwrap_or(false)
                    } else {
                        false
                    };
                    
                    if !has_real_selection {
                        // Just a click without drag - cancel the empty selection
                        if let Ok(mut input) = self.input.try_borrow_mut() {
                            input.cancel_selection();
                        }
                    }
                    // Either way, transition to Active state
                    self.set_state(ButtonState::Active);
                }
            }
            MouseEventKind::Moved => {
                // Only change state on hover if we're not active or selecting.
                if !self.is_active() && current_state != ButtonState::Selecting {
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

impl <'a> WidgetRef for InputField <'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (_background, text_color, _shadow, highlight) = self.colors();
        
        // Ensure consistent background
        buf.set_style(area, Style::default().bg(THEME.bg));
        
        // Check if area is too small to render properly
        if area.height < INPUT_FIELD_MIN_HEIGHT {
            // If height is too small, render a compact version with just the title
            // This prevents the text from being invisible
            let compact_block = Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_type(BorderType::Rounded)
                .title(Line::raw(format!("{}: ", self.title)).fg(text_color))
                .style(Style::default().fg(highlight).bg(THEME.bg));
            
            // Render the block
            compact_block.render(area, buf);
            
            // Try to render at least the first line of text
            if let Ok(input) = self.input.try_borrow() {
                if let Some(first_line) = input.lines().first() {
                    let title_len = self.title.len() + 3; // ": " and left border
                    let text_area = Rect {
                        x: area.x + 1 + title_len as u16,
                        y: area.y,
                        width: area.width.saturating_sub(2 + title_len as u16),
                        height: 1,
                    };
                    // Truncate if needed
                    let display_text: String = first_line.chars().take(text_area.width as usize).collect();
                    buf.set_string(text_area.x, text_area.y, &display_text, Style::default().fg(CATPPUCCIN.text).bg(THEME.bg));
                }
            }
            
            self.set_area(area);
            return;
        }
        
        // Save the area for later use.
        self.set_area(area);
        self.check_text_wrapping(area);

        // Draw a bordered block with the field's title.
        let default_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::raw(self.title).fg(text_color))
            .style(Style::default().fg(highlight).bg(THEME.bg));

        let input = self.input.try_borrow_mut();

        if let Ok(mut input) = input {
            let block = if let Some(block) = self.block.borrow().clone(){
                block.style(Style::default().fg(highlight).bg(THEME.bg))
            } 
            else { 
                default_block 
            };
            // Set background style on the text area itself
            input.set_style(Style::default().fg(CATPPUCCIN.text).bg(THEME.bg));
            input.set_block(block);
            input.render(area, buf);
        }
    }
}

// In ratatui 0.30, f.render_widget requires Widget trait, not just WidgetRef
// Implement Widget for &InputField to allow f.render_widget(&input_field, area)
impl<'a> Widget for &InputField<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
}
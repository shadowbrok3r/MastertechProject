use ratatui::{buffer::Buffer, crossterm::event::{KeyCode, KeyModifiers}, layout::{Position, Rect}, style::{Color, Style, Stylize}, text::Line, widgets::{Block, BorderType, Borders, Widget, WidgetRef}};
use crate::terminal_mode::{events::action_handler::{get_event_sender, WidgetEvent, WidgetId}, styling::{CATPPUCCIN, CATPPUCCINTHEME, APP_BACKGROUND}};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use super::{button::{ButtonState, Theme}, ButtonType};
use super::tui_textarea::{CursorMove, TextArea};
use std::{cell::RefCell, rc::Rc};
use crossbeam::channel::Sender;
use textwrap::refill;

/// Minimum height required for an InputField to be visible (borders + 1 line of text)
pub const INPUT_FIELD_MIN_HEIGHT: u16 = 3;

// ---------------------------------------------------------------------------
// InputField: A wrapper around tui_input::Input for our form fields.
#[derive(Clone, Debug)]
pub struct InputField <'a> {
    id: WidgetId,
    /// The underlying tui‑input state.
    pub input: RefCell<TextArea<'a>>,
    /// Title shown as the field’s label.
    title: &'static str,
    /// Store the last drawn area.
    area: RefCell<Option<Rect>>,
    /// state of input field
    state: RefCell<ButtonState>,
    /// duh
    theme: Theme,
    block: RefCell<Option<Block<'a>>>,
    event_sender: Sender<WidgetEvent>,
    has_wrapped: Rc<RefCell<bool>>,
    last_width: RefCell<Option<usize>>,
}

impl <'a> InputField <'a>{
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
            theme: CATPPUCCINTHEME,
            event_sender: get_event_sender(),
            has_wrapped: Rc::new(RefCell::new(false)),
            last_width: RefCell::new(None),
            
        }
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
        // Render the input's value in a Paragraph.
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

        // Check for width change and unwrap if necessary
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

        // Check for pasted text (optional fallback, can remove if using insert_wrapped_text)
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
        if let ButtonState::Active = *self.state.borrow() {
            true
        } else {
            false
        }
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

        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if inside {
                    // Toggle: if already active, set to Normal; otherwise, set to Active.
                    if self.is_active() {
                        self.set_state(ButtonState::Normal);
                    } else {
                        self.set_state(ButtonState::Active);
                        let _ = self.event_sender.try_send(WidgetEvent::Active { widget_id: self.id.clone() });
                    }
                } else {
                    self.set_state(ButtonState::Normal);
                }
            }
            MouseEventKind::Moved => {
                // Only change state on hover if we're not already active.
                if !self.is_active(){
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
        buf.set_style(area, Style::default().bg(APP_BACKGROUND));
        
        // Check if area is too small to render properly
        if area.height < INPUT_FIELD_MIN_HEIGHT {
            // If height is too small, render a compact version with just the title
            // This prevents the text from being invisible
            let compact_block = Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_type(BorderType::Rounded)
                .title(Line::raw(format!("{}: ", self.title)).fg(text_color))
                .style(Style::default().fg(highlight).bg(APP_BACKGROUND));
            
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
                    buf.set_string(text_area.x, text_area.y, &display_text, Style::default().fg(CATPPUCCIN.text).bg(APP_BACKGROUND));
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
            .style(Style::default().fg(highlight).bg(APP_BACKGROUND));

        let input = self.input.try_borrow_mut();

        if let Ok(mut input) = input {
            let block = if let Some(block) = self.block.borrow().clone(){
                block.style(Style::default().fg(highlight).bg(APP_BACKGROUND))
            } 
            else { 
                default_block 
            };
            // Set background style on the text area itself
            input.set_style(Style::default().fg(CATPPUCCIN.text).bg(APP_BACKGROUND));
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
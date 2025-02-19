use ratatui::{buffer::Buffer, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::{Color, Style, Stylize}, text::Line, widgets::{Block, BorderType, Borders, Paragraph, Widget, WidgetRef}, Frame};
use crate::{fx::{effect::{selected_category, UniqueEffectId}, EffectStage}, styling::{CATPPUCCIN, CATPPUCCINTHEME, DEEPPINK, TURQUOISE}, widgets::SHORTCUT_SET};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use super::{button::{Button, State, Theme}, ButtonType, ShrinkArea};
use tui_input::{backend::crossterm::EventHandler, Input};
use tui_textarea::TextArea;
use std::cell::RefCell;


// ---------------------------------------------------------------------------
// InputField: A wrapper around tui_input::Input for our form fields.
#[derive(Clone, Debug)]
pub struct InputField <'a> {
    /// The underlying tui‑input state.
    pub input: RefCell<TextArea<'a>>,
    /// Title shown as the field’s label.
    title: &'static str,
    /// Store the last drawn area.
    area: RefCell<Option<Rect>>,
    /// state of input field
    state: RefCell<State>,
    /// duh
    theme: Theme,
    effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    init: RefCell<bool>,
    block: RefCell<Option<Block<'a>>>,
}

impl <'a> InputField <'a>{
    pub fn new(title: &'static str) -> Self {
        let mut text_area = TextArea::default();
        text_area.set_style(Style::default().fg(CATPPUCCIN.text));

        let input = RefCell::new(text_area);

        Self {
            input,
            title,
            block: RefCell::new(None),
            area: RefCell::new(None),
            state: RefCell::new(State::Normal),
            theme: CATPPUCCINTHEME,
            effect_stage: RefCell::new(EffectStage::default()),
            init: RefCell::new(true),
        }
    }

    fn set_cursor(&self) {
        let mut input = self.input.borrow_mut();
        match *self.state.borrow() {
            State::Active => input.set_cursor_style(Style::default().fg(Color::Cyan)),
            _ => input.set_cursor_style(Style::default().hidden())
        }
    }

    fn add_effect(&self, area: Rect) {
        if *self.init.borrow() {
            *self.init.borrow_mut() = false;
            let effect1 = selected_category(Color::LightRed, area);
            self.effect_stage.borrow_mut().add_effect(effect1);
        }
    }

    fn set_block(&self, block: Block<'a>) {
        self.block.replace(Some(block));
    }
}

impl <'a> WidgetRef for InputField <'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (background, text_color, shadow, highlight) = self.colors();
        
        self.add_effect(area);
        
        // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        let fx_duration = tachyonfx::Duration::from_millis(16);
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        self.effect_stage.borrow_mut().process_effects(fx_duration, buf, *buf.area());

        // buf.set_style(area, Style::default().bg(background).fg(text_color));
        // Save the area for later use.
        self.set_area(area);
        
        // Draw a bordered block with the field’s title.
        let default_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::raw(self.title).fg(text_color))
            .style(Style::default().fg(highlight));

        // Render the input's value in a Paragraph.
        let mut input = self.input.borrow_mut();
        let block = if let Some(block) = self.block.borrow().clone(){
            block
        } 
        else { 
            default_block 
        };

        input.set_block(block);
        input.render(area, buf);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFieldId {
    CustomerName,
    CustomerPhone,
    SalesmanName,
    TechnicianName,
    CheckInNotes,
    Recommendations,
    ServiceNumber,
}

impl<'a> ButtonType<'a> for InputField <'a> {
    fn on_click(&self, _f: impl FnMut() + 'a) -> Self {
        // You might not need on_click for an input field.
        self.clone()
    }

    fn click(&self) {
        self.set_state(State::Active);
    }

    fn set_state(&self, state: State) {
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
        if let State::Active = *self.state.borrow() {
            true
        } else {
            false
        }
    }

    /// Helper method to get the right colors based on the current state.
    fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme;
        match *self.state.borrow() {
            State::Normal => (CATPPUCCIN.lavender, t.text, t.shadow, t.highlight),
            State::Selected => (CATPPUCCIN.blue, CATPPUCCIN.text, CATPPUCCIN.sapphire, CATPPUCCIN.red),
            State::Active => (CATPPUCCIN.green, CATPPUCCIN.teal, CATPPUCCIN.red, CATPPUCCIN.blue),
            State::Hovered => (CATPPUCCIN.lavender, CATPPUCCIN.sapphire, CATPPUCCIN.red, CATPPUCCIN.maroon),
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
                        self.set_state(State::Normal);
                    } else {
                        self.set_state(State::Active);
                    }
                } else {
                    self.set_state(State::Normal);
                }
            }
            MouseEventKind::Moved => {
                // Only change state on hover if we're not already active.
                if !self.is_active(){
                    if inside {
                        self.set_state(State::Hovered);
                    } else {
                        self.set_state(State::Normal);
                    }
                }
            }
            _ => {}
        }
    }

}

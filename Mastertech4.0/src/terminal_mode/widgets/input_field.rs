use crossbeam::channel::Sender;
use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Style, Stylize}, text::Line, widgets::{Block, BorderType, Borders, Widget, WidgetRef}};
use crate::terminal_mode::{events::action_handler::{get_event_sender, WidgetEvent, WidgetId}, fx::{effect::{selected_category, UniqueEffectId}, EffectStage}, styling::{CATPPUCCIN, CATPPUCCINTHEME}};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use super::{button::{ButtonState, Theme}, ButtonType};
use tui_textarea::TextArea;
use std::cell::RefCell;

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
    effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    init: RefCell<bool>,
    block: RefCell<Option<Block<'a>>>,
    event_sender: Sender<WidgetEvent>,
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
            effect_stage: RefCell::new(EffectStage::default()),
            init: RefCell::new(true),
            event_sender: get_event_sender()
        }
    }

    fn set_cursor(&self) {
        let mut input = self.input.borrow_mut();
        match *self.state.borrow() {
            ButtonState::Active => input.set_cursor_style(Style::default().fg(Color::Cyan)),
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

    pub fn set_block(&self, block: Block<'a>) {
        self.block.replace(Some(block));
    }
}

impl <'a> WidgetRef for InputField <'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (_background, text_color, _shadow, highlight) = self.colors();
        
        self.add_effect(*buf.area());
        
        // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        // let fx_duration = tachyonfx::Duration::from_millis(16);
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        // self.effect_stage.borrow_mut().process_effects(fx_duration, buf, *buf.area());

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
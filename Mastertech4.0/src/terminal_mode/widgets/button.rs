use crossbeam::channel::Sender;
use ratatui::{
    buffer::Buffer, crossterm::event::{MouseButton, MouseEvent, MouseEventKind}, layout::{Position, Rect}, style::{Color, Style}, text::Line, widgets::{Widget, WidgetRef}
};
use tachyonfx::{CellFilter, Effect};
use crate::{filesystem::get_client_hash, terminal_mode::{events::action_handler::{get_event_sender, WidgetButton, WidgetEvent, WidgetId}, fx::{effect::{outline_selected_cells, UniqueEffectId}, EffectStage}, styling::{TURQUOISE, APP_BACKGROUND}}};
use std::{cell::RefCell, fmt::{Debug, Display}};
use super::{ButtonType, SHORTCUT_SET};
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};


/// ------------------------------
/// Custom Button widget
/// ------------------------------
/// Holds info for each button:
/// - `label`: what text to display
/// - `state`: normal, selected, active, etc.
/// - `theme`: coloring for the button
/// - `area`: updated at runtime (where the button was drawn)
/// - `shrink`: The size to shrink the button by, 
///     to make the button not take the entire area it resides in
/// - `on_click`: optional callback to do something when the button is clicked
#[derive(Clone, Debug)]
pub struct Button<'a> {
    id: WidgetId,
    title: String,
    label: Line<'a>,
    theme: Theme,
    state: RefCell<ButtonState>,
    // on_click: Arc<RefCell<Option<Box<dyn FnMut() + 'a>>>>,
    // on_click: Arc<RefCell<Option<F>>>,
    area: RefCell<Option<Rect>>,
    effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    init: RefCell<bool>,
    event_sender: Sender<WidgetEvent>,
    /// If true, this button acts as a tab button - effects only show when selected
    is_tab_button: bool,
    /// If true, this tab is currently selected (only relevant for tab buttons)
    is_selected_tab: RefCell<bool>,
    /// Whether to show animated effects on this button
    effects_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonState {
    #[default]
    Normal,
    Hovered,
    Selected,
    Active,
    AltClicked,
    /// Used when user is dragging to select text in an InputField
    Selecting,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub text: Color,
    pub background: Color,
    pub highlight: Color,
    pub shadow: Color,
}

impl<'a> Button<'a> {
    pub fn new(label: impl Display, id: WidgetId) -> Self {
        Button {
            id,
            title: label.to_string(),
            label: Line::raw(label.to_string()),
            theme: TURQUOISE,
            state: RefCell::new(ButtonState::Normal),
            area: RefCell::new(None),
            effect_stage: RefCell::new(EffectStage::default()),
            init: RefCell::new(true),
            event_sender: get_event_sender(),
            is_tab_button: false,
            is_selected_tab: RefCell::new(false),
            effects_enabled: true,
        }
    }

    pub fn get_widget_id(&self) -> WidgetId {
        self.id.clone()
    }

    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
    
    /// Mark this button as a tab button - effects will only show when selected
    pub fn as_tab(mut self) -> Self {
        self.is_tab_button = true;
        self
    }
    
    /// Enable or disable effects on this button
    pub fn with_effects(mut self, enabled: bool) -> Self {
        self.effects_enabled = enabled;
        self
    }
    
    /// Set whether this tab is currently selected (only for tab buttons)
    pub fn set_selected(&self, selected: bool) {
        *self.is_selected_tab.borrow_mut() = selected;
        // Reset init to trigger effect recreation with new area if now selected
        if selected {
            *self.init.borrow_mut() = true;
        }
    }
    
    /// Check if this tab button is currently selected
    pub fn is_selected(&self) -> bool {
        *self.is_selected_tab.borrow()
    }
    
    /// Returns true if effects should be shown for this button
    fn should_show_effects(&self) -> bool {
        if !self.effects_enabled {
            return false;
        }
        
        if self.is_tab_button {
            // Tab buttons only show effects when selected
            *self.is_selected_tab.borrow()
        } else {
            // Regular buttons show effects in Active or Hovered state
            matches!(*self.state.borrow(), ButtonState::Active | ButtonState::Hovered | ButtonState::Selected)
        }
    }

    pub fn _set_effect(&self, effect: Effect) {
        self.effect_stage.borrow_mut().add_effect(effect);
    }

    pub fn set_label(&mut self, label: String) {
        self.title = label.clone();
        self.label = Line::raw(label.clone());
    }

    pub fn get_label(&self) -> &str {
        &self.title
    }

    // pub fn _on_click(self, f: impl FnMut() + 'a) -> Self {
    //     self.on_click.replace(Some(Box::new(f)));
    //     self
    // }
}

impl <'a> ButtonType<'a> for Button<'a> {
    
    // fn click(&self) {
        // if let Some(callback) = self.on_click.borrow_mut().as_mut() {
        //     log::info!("click callback fired");
        //     callback(); // Error here because i dont pass an arg to callback(*)
        // }
    // }
    
    fn set_state(&self, state: ButtonState) {
        self.state.replace(state);
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
            ButtonState::Normal => (t.background, t.text, t.shadow, t.highlight),
            ButtonState::Selected => (t.background, t.text, Color::White, Color::White),
            ButtonState::Active => (t.background, t.text, t.highlight, t.shadow),
            ButtonState::Hovered => (t.background, t.text, t.highlight, t.shadow),
            ButtonState::AltClicked => (t.background, t.text, Color::White, Color::White),
            ButtonState::Selecting => (t.background, t.text, t.highlight, t.shadow),
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // If we haven’t assigned an area yet, do nothing
        let Some(area) = self.get_area() else { return; };
        let c = mouse_event.column;
        let r = mouse_event.row;
        let mouse_position = Position::new(c, r);

        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if the mouse click is within area
                if area.contains(mouse_position) {
                    self.set_state(ButtonState::Active);
                    let _ = self.event_sender.try_send(WidgetEvent::ButtonClick { widget_id: self.id.clone(), button: WidgetButton::Left, source: get_client_hash().connection_string });
                    self.click(); // calls our on_click callback
                } else {
                    self.set_state(ButtonState::Normal);
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if area.contains(mouse_position) {
                    let _ = self.event_sender.try_send(WidgetEvent::ButtonClick { widget_id: self.id.clone(), button: WidgetButton::Right, source: get_client_hash().connection_string });
                    self.set_state(ButtonState::AltClicked);
                } else {
                    self.set_state(ButtonState::Normal);
                }
            }
            MouseEventKind::Moved => {
                // If you want hover behavior, do it here
                if area.contains(mouse_position) {
                    // self.event_sender.try_send(WidgetEvent::Hover { widget_id: self.id });
                    self.set_state(ButtonState::Selected);
                } else {
                    self.set_state(ButtonState::Normal);
                }
            }
            _ => {}
        }
    }
}

impl <'a> WidgetRef for Button<'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();

        buf.set_style(area, Style::default().fg(text).bg(APP_BACKGROUND));
        
        // Horizontal padding inside the button (space between text and borders)
        const HORIZONTAL_PADDING: u16 = 1;
        
        // Draw top border with rounded corners
        if area.height > 1 {
            let mut top_str = String::new();
            top_str.push_str(&SHORTCUT_SET.top_left);
            top_str.push_str(&SHORTCUT_SET.horizontal_top.repeat(area.width.saturating_sub(2) as usize));
            top_str.push_str(&SHORTCUT_SET.top_right);

            buf.set_string(
                area.x,
                area.y,
                top_str,
                Style::default().fg(highlight).bg(APP_BACKGROUND),
            );
        }

        // Calculate inner height properly
        let inner_start = area.y + 1; // Start just below the top border
        let inner_end = area.y + area.height.saturating_sub(2); // Stop before the bottom border

        // Draw side borders & apply background correctly
        for y in inner_start..=inner_end {
            // Left border
            buf.set_string(
                area.x,
                y,
                SHORTCUT_SET.vertical_left,
                Style::default().fg(highlight).bg(APP_BACKGROUND),
            );

            // Background fill (inside the borders)
            let inner_rect = Rect {
                x: area.x + 1,
                y,
                width: area.width.saturating_sub(2),
                height: 1,  // Exactly 1 row per iteration
            };
            buf.set_style(inner_rect, Style::default().fg(text).bg(APP_BACKGROUND));

            // Right border
            buf.set_string(
                area.x + area.width - 1,
                y,
                SHORTCUT_SET.vertical_right,
                Style::default().fg(highlight).bg(APP_BACKGROUND),
            );
        }

        // Draw bottom border with rounded corners
        if area.height > 1 {
            let mut bot_str = String::new();
            bot_str.push_str(&SHORTCUT_SET.bottom_left);
            bot_str.push_str(&SHORTCUT_SET.horizontal_bottom.repeat(area.width.saturating_sub(2) as usize));
            bot_str.push_str(&SHORTCUT_SET.bottom_right);

            buf.set_string(
                area.x,
                area.y + area.height - 1,
                bot_str,
                Style::default().fg(shadow).bg(APP_BACKGROUND),
            );
        }

        // Calculate available inner width for the label (subtract borders and padding)
        let inner_width = area.width.saturating_sub(2 + HORIZONTAL_PADDING * 2) as usize;
        
        // Get the label text and truncate if necessary
        let label_text = self.title.clone();
        let label_width = label_text.width();
        
        let display_text = if label_width > inner_width && inner_width > 3 {
            // Truncate and add ellipsis
            let mut truncated = String::new();
            let mut current_width = 0;
            for ch in label_text.chars() {
                let ch_width = ch.width().unwrap_or(1);
                if current_width + ch_width + 2 > inner_width { // +2 for ".."
                    break;
                }
                truncated.push(ch);
                current_width += ch_width;
            }
            truncated.push_str("..");
            truncated
        } else if label_width > inner_width {
            // Too narrow for even truncation, show what we can
            label_text.chars().take(inner_width.saturating_sub(1)).collect::<String>()
        } else {
            label_text
        };
        
        // Create a new Line with the (potentially truncated) text
        let display_line = Line::raw(display_text.clone());
        let display_width = display_text.width() as u16;
        
        // Center the label inside the background (accounting for borders + padding)
        let available_width = area.width.saturating_sub(2); // Just borders
        let label_x = area.x + 1 + (available_width.saturating_sub(display_width)) / 2;
        let label_y = area.y + (area.height.saturating_sub(2)) / 2 + 1; // Adjust to align inside the box
        
        // Only render if we have valid coordinates within the area
        if label_y > area.y && label_y < area.y + area.height - 1 {
            buf.set_line(label_x, label_y, &display_line, available_width);
        }

        self.set_area(area);
        
        // Only process effects if they should be shown
        if self.should_show_effects() {
            let mut init = self.init.borrow_mut();
            if *init {
                *init = false;
                let mut effect_stage = self.effect_stage.borrow_mut();
                
                if self.is_tab_button {
                    // For tab buttons, use the selected_tab_effect
                    use crate::terminal_mode::fx::effect::selected_tab_effect;
                    let effect = selected_tab_effect(&mut effect_stage, background, area);
                    effect_stage.add_effect(effect);
                } else {
                    // For regular buttons, use the outline effect
                    let effect1 = outline_selected_cells(
                        &mut effect_stage, 
                        area.as_size(),
                        background,
                        CellFilter::FgColor(Color::White)
                    );
                    effect_stage.add_effect(effect1);
                }
            }

            let fx_duration = tachyonfx::Duration::from_millis(16);
            self.effect_stage.borrow_mut().process_effects(fx_duration, buf, area);
        }
    }
}

// In ratatui 0.30, f.render_widget requires Widget trait, not just WidgetRef
// Implement Widget for &Button to allow f.render_widget(&button, area)
impl<'a> Widget for &Button<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
}
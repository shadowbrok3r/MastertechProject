use ratatui::{
    buffer::Buffer, crossterm::event::{MouseButton, MouseEvent, MouseEventKind}, layout::Rect, style::{Color, Style}, text::Line, widgets::WidgetRef
};
use crate::terminal_mode::styling::TURQUOISE;
use std::{cell::RefCell, fmt::Debug, sync::Arc};
use super::{ButtonType, SHORTCUT_SET};


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
#[derive(Clone)]
pub struct Button<'a> {
    title: &'a str,
    label: Line<'a>,
    theme: Theme,
    state: RefCell<State>,
    on_click: Arc<RefCell<Option<Box<dyn FnMut() + 'a>>>>,
    // on_click: Arc<RefCell<Option<F>>>,
    area: RefCell<Option<Rect>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Normal,
    Hovered,
    Selected,
    Active
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub text: Color,
    pub background: Color,
    pub highlight: Color,
    pub shadow: Color,
}

/*
pub struct Button<'a, F>
where 
    F: FnMut(),

impl<'a, F> Button<'a, F> 
where 
    F: FnMut(),
{
    pub fn new<T: Into<Line<'a>>>(label: T) -> Self 
    where
        F: Default,
impl <'a, F: FnMut()> ButtonType<'a> for Button<'a, F> {
*/

impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Button {
            title: label.clone(),
            label: Line::raw(label),
            theme: TURQUOISE,
            state: RefCell::new(State::Normal),
            area: RefCell::new(None),
            on_click: Arc::new(RefCell::new(None)),
        }
    }

    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn set_label(&mut self, label: String) {
        self.label = Line::raw(label);
    }

    pub fn get_label(&self) -> &str {
        &self.title
    }

    pub fn on_click(self, f: impl FnMut() + 'a) -> Self {
        self.on_click.replace(Some(Box::new(f)));
        self
    }
}

impl <'a> ButtonType<'a> for Button<'a> {
    /// For when something (mouse, keyboard) triggers a "click" on this Button.
    fn click(&self) {
        if let Some(callback) = self.on_click.borrow_mut().as_mut() {
            log::info!("click callback fired");
            callback(); // Call the callback
        }
    }
    
    fn set_state(&self, state: State) {
        self.state.replace(state);
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
            State::Normal => (t.background, t.text, t.shadow, t.highlight),
            State::Selected => (t.highlight, t.text, t.shadow, t.highlight),
            State::Active => (t.background, t.text, t.highlight, t.shadow),
            State::Hovered => (t.background, t.text, t.highlight, t.shadow),
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // If we haven’t assigned an area yet, do nothing
        let Some(area) = *self.area.borrow() else { return; };

        
        let c = mouse_event.column;
        let r = mouse_event.row;
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // log::info!("Handling Button press in rect: {area:?}\nMouse event column/row: {c}/{r}");
                // Check if the mouse click is within area
                if c >= area.x && c < area.x + area.width &&
                   r >= area.y && r < area.y + area.height {
                    self.set_state(State::Active);
                    self.click(); // calls our on_click callback
                } else {
                    self.set_state(State::Normal);
                }
            }
            MouseEventKind::Moved => {
                // If you want hover behavior, do it here
                if c >= area.x && c < area.x + area.width &&
                   r >= area.y && r < area.y + area.height {
                    self.set_state(State::Selected);
                } else {
                    self.set_state(State::Normal);
                }
            }
            _ => {}
        }
    }
}

// impl ratatui::widgets::StatefulWidgetRef for Button {type State;}

impl <'a> WidgetRef for Button<'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();

        // let block = Block::default()
        //     .fg(text)
        //     .borders(Borders::ALL)
        //     .border_set(SHORTCUT_SET)
        //     // .border_type(ratatui::widgets::BorderType::Rounded)
        //     .border_style(Style::default().blue());

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
                Style::default().fg(highlight).bg(Color::Reset), // No background spill
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
                Style::default().fg(highlight).bg(Color::Reset),
            );

            // Background fill (inside the borders)
            let inner_rect = Rect {
                x: area.x + 1,
                y,
                width: area.width.saturating_sub(2),
                height: 1,  // Exactly 1 row per iteration
            };
            buf.set_style(inner_rect, Style::default().bg(background).fg(text));

            // Right border
            buf.set_string(
                area.x + area.width - 1,
                y,
                SHORTCUT_SET.vertical_right,
                Style::default().fg(highlight).bg(Color::Reset),
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
                Style::default().fg(shadow).bg(Color::Reset),
            );
        }

        // Center the label inside the background
        let label_x = area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2;
        let label_y = area.y + (area.height.saturating_sub(2)) / 2 + 1; // Adjust to align inside the box
        buf.set_line(label_x, label_y, &self.label, area.width);

        self.set_area(area);
    }
}


/*


impl <'a> Widget for Button<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        // Fill area with background + text color.
        buf.set_style(area, Style::default().bg(background).fg(text));

        // If there's room, draw top highlight line.
        if area.height > 2 {
            let mut top_str = String::new();
            top_str.insert(0, '┌'); 
            /* ┐ ┬ └ ┘ └ ┘ */
            // top_str.insert(0, '┌');
            let top_str = "▔".repeat(area.width as usize);
            
            buf.set_string(
                area.x,
                area.y,
                top_str,
                Style::default().fg(highlight).bg(background),
            );
        }
        // If there's room, draw bottom shadow line.
        if area.height > 1 {
            let mut bot_str = "▁".repeat(area.width as usize - 1);
            bot_str.push_str("◢");
            
            buf.set_string(
                area.x,
                area.y + area.height - 1,
                bot_str,
                Style::default().fg(shadow).bg(background),
            );
        }

        // Center the label.
        let label_x = area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2;
        let label_y = area.y + (area.height.saturating_sub(1)) / 2;
        buf.set_line(label_x, label_y, &self.label, area.width);
    }
}
    
use super::{colors::{Theme, BLUE}, State};
use ratatui::{
    style::{Color, Style},
    buffer::Buffer, layout::Rect, text::Line, widgets::Widget
};

#[derive(Debug, Clone)]
pub struct Button<'a> {
    pub label: Line<'a>,
    pub theme: Theme,
    pub state: State,
}


/// A button with a label that can be themed.
impl<'a> Button<'a> {
    pub fn new<T: Into<Line<'a>>>(label: T) -> Self {
        Button {
            label: label.into(),
            theme: BLUE,
            state: State::Normal,
        }
    }

    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }
}

impl<'a> Widget for Button<'a> {
    #[allow(clippy::cast_possible_truncation)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (background, text, shadow, highlight) = self.colors();
        buf.set_style(area, Style::new().bg(background).fg(text));

        // render top line if there's enough space
        if area.height > 2 {
            buf.set_string(
                area.x,
                area.y,
                "▔".repeat(area.width as usize),
                Style::new().fg(highlight).bg(background),
            );
        }
        // render bottom line if there's enough space
        if area.height > 1 {
            buf.set_string(
                area.x,
                area.y + area.height - 1,
                "▁".repeat(area.width as usize),
                Style::new().fg(shadow).bg(background),
            );
        }
        // render label centered
        buf.set_line(
            area.x + (area.width.saturating_sub(self.label.width() as u16)) / 2,
            area.y + (area.height.saturating_sub(1)) / 2,
            &self.label,
            area.width,
        );
    }
}

impl Button<'_> {
    const fn colors(&self) -> (Color, Color, Color, Color) {
        let theme = self.theme;
        match self.state {
            State::Normal => (theme.background, theme.text, theme.shadow, theme.highlight),
            State::Selected => (theme.highlight, theme.text, theme.shadow, theme.highlight),
            State::Active => (theme.background, theme.text, theme.highlight, theme.shadow),
        }
    }
}

*/
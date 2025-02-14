use ratatui::{crossterm::event::{MouseButton, MouseEvent, MouseEventKind}, layout::Rect, prelude::Backend, style::{Color, Style}, symbols, text::Span, widgets::{Block, Borders, Tabs}, Frame};
use crate::terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::{CATPPUCCIN, C_DEEPPINK}, widgets::{HandleWidget, SHORTCUT_SET_2}};
use unicode_width::UnicodeWidthStr;
pub use service_order::*;
pub use scripts::*;
pub use sysinfo::*;

pub mod scripts;
pub mod service_order;
pub mod sysinfo;

/// All of our tabs to be layed out in main
/// menu bar
pub const TABS: [&str; 3] = [
    "Ticket", 
    "Scripts", 
    "System",
];

////////////////////////////////////
/// TABS FOR MENU BAR
////////////////////////////////////
#[derive(Debug, Clone, Copy, Default)]
pub enum Tab {
    #[default]
    TurSheet,
    Scripts,
    SystemInfo,
}

////////////////////////////////
/// MENU BAR
////////////////////////////////
pub struct MenuBar<'a> {
    hovered_index: Option<usize>,
    pub selected_tab: Tab,
    pub tabs: Tabs<'a>,
    on_clicks: Vec<Box<dyn FnMut() + 'a>>,
    selected_index: usize,
    tab_bounding_boxes: Vec<Rect>,
    // Copy the same fields used by the Tabs widget:
    divider: Span<'a>,
    padding_left: Span<'a>,
    padding_right: Span<'a>,
    /// Titles of each tab (so we know how many bounding boxes to generate)
    titles: Vec<&'a str>,
    pub effect_stage: EffectStage<UniqueEffectId>,
}

impl<'a> MenuBar<'a> {
    pub fn new() -> Self {
        let divider = Span::raw(symbols::DOT); // or symbols::DOT
        let padding_left = Span::raw("  ");
        let padding_right = Span::raw("  ");

        // let base_color = CATPPUCCIN.flamingo;
        // let bg_color = CATPPUCCIN.base.lerp(&Color::Black, 0.15);
        // let border_color = CATPPUCCIN.base.lerp(&base_color, 0.85);
        // let border_style = Style::default().fg(border_color);
        // Render Tabs at top
        let tabs = Tabs::new(TABS)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .title_alignment(ratatui::layout::Alignment::Center)
                    .border_set(SHORTCUT_SET_2)

            )
            .padding(padding_left.clone(), padding_right.clone())
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().fg(C_DEEPPINK))
            .divider(divider.clone());

        let on_clicks = vec![
            Box::new(|| {}) as Box<dyn FnMut()>,
            Box::new(|| {}),
            Box::new(|| {}),
        ];

        let mut menu_bar = Self {
            hovered_index: None,
            titles: TABS.to_vec(),
            tabs,
            on_clicks,
            selected_index: 0,
            tab_bounding_boxes: Vec::new(),
            selected_tab: Tab::TurSheet,
            divider,
            padding_left,
            padding_right,
            effect_stage: EffectStage::default(),
        };

        menu_bar.on_tab_click(0, || {
            log::info!("Clicked TUR Sheet tab");
        });
        menu_bar.on_tab_click(1, move || {
            log::info!("Clicked Scripts tab");
        });
        menu_bar.on_tab_click(2, || {
            log::info!("Clicked Sysinfo tab");
        });

        menu_bar
    }

    /// Let the caller register a callback for a specific tab,
    ///  but DO NOT treat it as a real click event.
    pub fn on_tab_click(
        &mut self,
        tab_idx: usize,
        cb: impl FnMut() + 'a
    ) {
        if let Some(slot) = self.on_clicks.get_mut(tab_idx) {
            *slot = Box::new(cb);
        }
    }

    /// Convert the numeric index to your `Tab` enum
    pub fn selected_tab(&self) -> Tab {
        match self.selected_index {
            0 => Tab::TurSheet,
            1 => Tab::Scripts,
            2 => Tab::SystemInfo,
            _ => Tab::TurSheet,
        }
    }

    /// Set which tab is active
    pub fn set_selected(&mut self, index: usize) {
        self.selected_index = index;
        let _ = self.tabs.clone().select(index);
    }

    /// Highlights selected tab
    pub fn highlight_selected(&mut self, index: usize) {
        self.selected_index = index;
        let _ = self.tabs.clone().select(index);
    }
}

impl <'a> HandleWidget <'_> for MenuBar <'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {        
        // 1) Update the Tabs to reflect our currently selected index
        let updated_tabs = self.tabs.clone();
        let tabs = updated_tabs.select(self.selected_index);
        self.tabs = tabs;
        // 2) Clear out old bounding boxes
        self.tab_bounding_boxes.clear();
    
        // 3) Compute each tab’s bounding box based on padding, title widths, and divider widths.
        let mut x = area.left();
        let right_bound = area.right();
        let titles_len = self.titles.len();
        for (i, title_str) in self.titles.iter().enumerate() {
            if x >= right_bound {
                break;
            }
            let start_x = x;
            let left_pad_width = self.padding_left.width();
            let right_pad_width = self.padding_right.width();
            let divider_width = if i < titles_len - 1 {
                self.divider.width()
            } else {
                0
            };
            let title_width = title_str.width();
            x = x.saturating_add(left_pad_width as u16);
            x = x.saturating_add(title_width as u16);
            x = x.saturating_add(right_pad_width as u16);
            if i < titles_len - 1 {
                x = x.saturating_add(divider_width as u16);
            }
            let actual_width = x.saturating_sub(start_x).min(right_bound.saturating_sub(start_x));
            self.tab_bounding_boxes.push(Rect {
                x: start_x,
                y: area.top(),
                width: actual_width,
                height: area.height,
            });
        }
    
        // 4) Render the actual Tabs widget over the entire area
        f.render_widget(&self.tabs, area);
    
        // 5) Draw a frame (Block) around each tab bounding box
        for (i, rect) in self.tab_bounding_boxes.iter().enumerate() {

            let style = if i == self.selected_index {
                Style::default().fg(CATPPUCCIN.pink)
            } else {
                Style::default().fg(CATPPUCCIN.green)
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(style);

            f.render_widget(block, *rect);
        }

                // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        let fx_duration = tachyonfx::Duration::from_millis(16);
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        self.effect_stage.process_effects(fx_duration, f.buffer_mut(), area);

    }
    

    fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = self.hovered_index {
                    self.set_selected(i);  // Actual selection
                    // Optionally call the callback if you want
                    if let Some(cb) = self.on_clicks.get_mut(i) {
                        (cb)();
                    }
                }
            }
            MouseEventKind::Moved => {
                let (c, r) = (mouse_event.column, mouse_event.row);
                // Figure out which tab (if any) the mouse is over
                let mut hovered = None;
                // 1) Loop over all bounding boxes
                for (i, rect) in self.tab_bounding_boxes.iter().enumerate() {
                    // Create an expanded version of the area rect 
                    // to detect the mouse going out of bounds
                    // then default the selection back to the 
                    // currently viewed tab
                    let expanded_rect = Rect {
                        x: rect.x.saturating_sub(2),
                        y: rect.y.saturating_sub(1),
                        width: rect.width.saturating_add(4),
                        height: rect.height.saturating_add(2),
                    };

                    let in_bounds = 
                        c >= expanded_rect.x
                        && c < expanded_rect.x + expanded_rect.width
                        && r >= expanded_rect.y
                        && r < expanded_rect.y + expanded_rect.height;

                    if in_bounds {
                        hovered = Some(i);
                        // found our hovered box, no need to keep searching
                        break;
                    }
                }

                // I am only checking if hovered is some right now
                // so we can determine whether the mouse has *left* 
                // the bounding box area of the button, so we can
                // highlight the actual tab we are on so the hovered 
                // highlighted state doesnt stick
                // 2) If we never found a match, hovered stays None.
                if let Some(i) = hovered { self.highlight_selected(i); } 
                else { } 

                // 3) Actually store it in `self.hovered_index`
                self.hovered_index = hovered;
            }
            _ => {}
        }
    }
}

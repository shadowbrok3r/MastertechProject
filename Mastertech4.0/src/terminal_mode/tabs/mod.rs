use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Offset, Rect}, prelude::Backend, widgets::WidgetRef, Frame};
use crate::terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::CATPPUCCINTHEME, widgets::{button::Button, ButtonType, HandleWidget, ShrinkArea}};
use std::cell::RefCell;
pub use service_order::*;
pub use scripts::*;
pub use sysinfo::*;

use super::{events::action_handler::WidgetId, styling::{CYAN, DARKORANGE, DEEPPINK, SPRINGGREEN}};

pub mod scripts;
pub mod service_order;
pub mod sysinfo;
pub mod logger;
pub mod login;

////////////////////////////////////
/// TABS FOR MENU BAR
////////////////////////////////////
#[derive(Debug, Clone, Copy, Default)]
pub enum Tab {
    #[default]
    TurSheet,
    Scripts,
    SystemInfo,
    Login,
    Logs
}

////////////////////////////////
/// MENU BAR
////////////////////////////////
#[derive(Clone)]
pub struct MenuBar<'a> {
    pub current_tab: RefCell<Tab>,
    pub effect_stage: EffectStage<UniqueEffectId>,
    ticket_tab: Button<'a>,
    scripts_tab: Button<'a>,
    system_tab: Button<'a>,
    logs_tab: Button<'a>,
    login_tab: Button<'a>,
}

impl<'a> MenuBar<'a> {
    pub fn new() -> Self {
        let menu_bar = Self {
            current_tab: RefCell::new(Tab::TurSheet),
            effect_stage: EffectStage::default(),
            ticket_tab: Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME),
            scripts_tab: Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN),
            system_tab: Button::new("System", WidgetId("System".to_owned())).theme(DEEPPINK),
            logs_tab: Button::new("Logs", WidgetId("Logs".to_owned())).theme(DARKORANGE),
            login_tab: Button::new("Login", WidgetId("Login".to_owned())).theme(SPRINGGREEN),
        };
        menu_bar
    }

    /// Convert the numeric index to your `Tab` enum
    pub fn selected_tab(&mut self) {
        let ticket_tab_active = self.ticket_tab.is_active();
        let system_tab_active = self.system_tab.is_active();
        let scripts_tab_active = self.scripts_tab.is_active();
        let logs_tab_active = self.logs_tab.is_active();
        let login_tab_active = self.login_tab.is_active();

        let new_tab = if ticket_tab_active {
            Tab::TurSheet
        } else if system_tab_active {
            Tab::SystemInfo
        } else if scripts_tab_active {
            Tab::Scripts
        } else if logs_tab_active {
            Tab::Logs
        } else if login_tab_active {
            Tab::Login
        } else {
            return;
        };

        self.current_tab.replace(new_tab);
    }

    pub fn set_active_tab(&self, tab: Tab) {
        self.current_tab.replace(tab);
    }
}

impl <'a> HandleWidget <'_> for MenuBar <'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {        
        // let style = if i == *self.selected_index.borrow() {Style::default().fg(CATPPUCCIN.rosewater)} 
        // else {Style::default().fg(CATPPUCCIN.blue)};
        // let block = Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Rounded).border_style(style);

        self.selected_tab();
        let row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 5),
                Constraint::Length(1),
                Constraint::Ratio(1, 5),
                Constraint::Length(1),
                Constraint::Ratio(1, 5),
                Constraint::Length(1),
                Constraint::Ratio(1, 5),
                Constraint::Length(1),
                Constraint::Ratio(1, 5),
            ])
            .split(area);

        let mut off = Offset::default();
        off.x = (area.width / 3) as i32;
        // let rec = area.offset(off);
        let ticket_tab_btn_area = row[0].shrink(1, 1);
        self.ticket_tab.render_ref(ticket_tab_btn_area, f.buffer_mut());

        let scripts_tab_area = row[2].shrink(0, 1);
        self.scripts_tab.render_ref(scripts_tab_area, f.buffer_mut());

        let system_tab_area = row[4].shrink(1, 1);
        self.system_tab.render_ref(system_tab_area, f.buffer_mut());

        let log_tab_area = row[6].shrink(1, 1);
        self.logs_tab.render_ref(log_tab_area, f.buffer_mut());

        let login_tab_area = row[8].shrink(1, 1);
        self.login_tab.render_ref(login_tab_area, f.buffer_mut());

        // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        self.effect_stage.process_effects(
            tachyonfx::Duration::from_millis(16), 
            f.buffer_mut(), 
            area
        );
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.ticket_tab.handle_mouse_event(&mouse_event);
        self.scripts_tab.handle_mouse_event(&mouse_event);
        self.system_tab.handle_mouse_event(&mouse_event);
        self.logs_tab.handle_mouse_event(&mouse_event);
        self.login_tab.handle_mouse_event(&mouse_event);
    }
}


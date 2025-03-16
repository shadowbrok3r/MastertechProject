use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::Stylize, widgets::{Block, Paragraph, WidgetRef, Wrap}, Frame};
use crate::terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::CATPPUCCINTHEME, widgets::{button::Button, ButtonType, HandleWidget, ShrinkArea}};
use std::{cell::RefCell, sync::{Arc, Mutex}};
use database::schema::User;
pub use scripts::*;
pub use sysinfo::*;

use super::{context::TerminalContext, events::action_handler::WidgetId, styling::{CATPPUCCIN, CYAN, DARKORANGE, DEEPPINK, SPRINGGREEN}};

pub mod scripts;
pub mod service_form;
pub mod tasks;
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
    Tasks,
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
    tasks_tab: Button<'a>,
    system_tab: Button<'a>,
    logs_tab: Button<'a>,
    pub login_tab: Button<'a>,
    ctx: Arc<Mutex<TerminalContext>>,
    client_title: String,
}

impl<'a> MenuBar<'a> {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let menu_bar = Self {
            current_tab: RefCell::new(Tab::TurSheet),
            effect_stage: EffectStage::default(),
            ticket_tab: Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME),
            scripts_tab: Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN),
            system_tab: Button::new("System", WidgetId("System".to_owned())).theme(DEEPPINK),
            tasks_tab: Button::new("Tasks", WidgetId("Tasks".to_owned())).theme(DEEPPINK),
            logs_tab: Button::new("Logs", WidgetId("Logs".to_owned())).theme(DARKORANGE),
            login_tab: Button::new("Login", WidgetId("Login".to_owned())).theme(SPRINGGREEN),
            ctx,
            client_title: String::new(),
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
        let tasks_tab_active = self.tasks_tab.is_active();

        let new_tab = if ticket_tab_active {
            Tab::TurSheet
        } else if system_tab_active {
            Tab::SystemInfo
        } else if scripts_tab_active {
            Tab::Scripts
        } else if tasks_tab_active {
            Tab::Tasks
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
                Constraint::Length(20),
                Constraint::Length(1),
                Constraint::Length(20),
                Constraint::Length(1),
                Constraint::Length(20),
                Constraint::Length(1),
                Constraint::Length(20),
                Constraint::Length(1),
                Constraint::Length(20),
                Constraint::Length(1),
                Constraint::Length(20),
                Constraint::Length(1),
                Constraint::Length(25),
            ])
            .split(area);


        self.ticket_tab.render_ref(row[0].shrink(1, 1), f.buffer_mut());
        self.scripts_tab.render_ref(row[2].shrink(1, 1), f.buffer_mut());
        self.tasks_tab.render_ref(row[4].shrink(1, 1), f.buffer_mut());
        self.system_tab.render_ref(row[6].shrink(1, 1), f.buffer_mut());
        self.logs_tab.render_ref(row[8].shrink(1, 1), f.buffer_mut());
        self.login_tab.render_ref(row[10].shrink(1, 1), f.buffer_mut());

        let title = &mut self.client_title;
        let user = &mut User::default();

        if user.name.is_empty() {
            if let Ok(ctx) = self.ctx.lock() {
                if !ctx.user.name.is_empty() {
                    *user = ctx.user.clone();
                }
            }
        }

        if title.is_empty() {
            if let Ok(ctx) = &self.ctx.lock() {
                *title = ctx.client_title.clone();
            }
        } else {
            Paragraph::new(format!("{}", &**title))
                .block(
                    Block::default()
                        .title_alignment(ratatui::layout::Alignment::Center)
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .fg(CATPPUCCIN.lavender)
                        .title(user.name.clone())
                )
                .right_aligned()
                .wrap(Wrap{ trim: false})
                .render_ref(row[12], f.buffer_mut());
        }
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
        self.tasks_tab.handle_mouse_event(&mouse_event);
        self.system_tab.handle_mouse_event(&mouse_event);
        self.logs_tab.handle_mouse_event(&mouse_event);
        self.login_tab.handle_mouse_event(&mouse_event);
    }
}


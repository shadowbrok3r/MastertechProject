pub mod scripts;
pub mod service_form;
pub mod tasks;
pub mod sysinfo;
pub mod logger;
pub mod login;
pub mod menu_bar;

pub use scripts::*;
pub use sysinfo::*;
pub use menu_bar::*;

// pub mod ncdu;


/*
use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::Stylize, widgets::{Block, Paragraph, WidgetRef, Wrap}, Frame};
use crate::{filesystem::get_client_hash, terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::CATPPUCCINTHEME, widgets::{button::Button, ButtonType, HandleWidget, ShrinkArea}}};
use std::{cell::RefCell, collections::HashMap, sync::{Arc, Mutex}};
use database::schema::User;
pub use scripts::*;
pub use sysinfo::*;
pub use tabs::*;

use super::{context::TerminalContext, events::action_handler::WidgetId, styling::{CATPPUCCIN, CYAN, DARKORANGE, DEEPPINK, SPRINGGREEN}};

pub mod scripts;
pub mod service_form;
pub mod tasks;
pub mod sysinfo;
pub mod logger;
pub mod login;
pub mod tabs;
// pub mod ncdu;

////////////////////////////////
/// MENU BAR
////////////////////////////////
#[derive(Clone)]
pub struct MenuBar<'a> {
    pub current_tab: RefCell<Tab>,
    pub effect_stage: EffectStage<UniqueEffectId>,
    pub tabs: HashMap<Tab, Button<'a>>,
    // ticket_tab: Button<'a>,
    // scripts_tab: Button<'a>,
    // tasks_tab: Button<'a>,
    // system_tab: Button<'a>,
    // ncdu_tab: Button<'a>,
    // logs_tab: Button<'a>,
    // pub login_tab: Button<'a>,
    ctx: Arc<Mutex<TerminalContext>>,
    client_title: String,
}

impl<'a> MenuBar<'a> {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let mut tabs = HashMap::new();
        tabs.insert(Tab::TurSheet, Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME));
        tabs.insert(Tab::Scripts, Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN));
        tabs.insert(Tab::Ncdu, Button::new("NCDU", WidgetId("Ncdu".to_owned())).theme(DEEPPINK));
        tabs.insert(Tab::Tasks, Button::new("Tasks", WidgetId("Tasks".to_owned())).theme(DARKORANGE));
        tabs.insert(Tab::SystemInfo, Button::new("System", WidgetId("System".to_owned())).theme(CATPPUCCINTHEME));
        tabs.insert(Tab::Logs, Button::new("Logs", WidgetId("Logs".to_owned())).theme(CYAN));
        tabs.insert(Tab::Login, Button::new("Login", WidgetId("Login".to_owned())).theme(SPRINGGREEN));

        let menu_bar = Self {
            current_tab: RefCell::new(Tab::TurSheet),
            effect_stage: EffectStage::default(),
            tabs,
            // ticket_tab: Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME),
            // scripts_tab: Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN),
            // system_tab: Button::new("System", WidgetId("System".to_owned())).theme(DEEPPINK),
            // ncdu_tab: Button::new("NCDU", WidgetId("Ncdu".to_owned())).theme(DARKORANGE),
            // tasks_tab: Button::new("Tasks", WidgetId("Tasks".to_owned())).theme(CATPPUCCINTHEME),
            // logs_tab: Button::new("Logs", WidgetId("Logs".to_owned())).theme(CYAN),
            // login_tab: Button::new("Login", WidgetId("Login".to_owned())).theme(SPRINGGREEN),
            ctx,
            client_title: String::new(),
        };
        menu_bar
    }

    // /// Convert the numeric index to your `Tab` enum
    // pub fn selected_tab(&mut self) {
    //     let ticket_tab_active = self.ticket_tab.is_active();
    //     let system_tab_active = self.system_tab.is_active();
    //     let scripts_tab_active = self.scripts_tab.is_active();
    //     let logs_tab_active = self.logs_tab.is_active();
    //     let login_tab_active = self.login_tab.is_active();
    //     let tasks_tab_active = self.tasks_tab.is_active();

    //     let new_tab = if ticket_tab_active {
    //         Tab::TurSheet
    //     } else if system_tab_active {
    //         Tab::SystemInfo
    //     } else if scripts_tab_active {
    //         Tab::Scripts
    //     } else if tasks_tab_active {
    //         Tab::Tasks
    //     } else if logs_tab_active {
    //         Tab::Logs
    //     } else if login_tab_active {
    //         Tab::Login
    //     } else {
    //         return;
    //     };

        // self.current_tab.replace(new_tab);
    // }

    pub fn set_active_tab(&self, tab: Tab) {
        self.current_tab.replace(tab);
    }

    // Update selected_tab to work with the HashMap if needed
    pub fn selected_tab(&mut self) {
        let mut current = self.current_tab.borrow_mut();
        for (tab, _button) in self.tabs.iter_mut() {
            if *tab == *current {
                *current = *tab;
            }
            // button.set_active(*tab == current);
        }
    }
}

impl <'a> HandleWidget <'_> for MenuBar <'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {        
        // let style = if i == *self.selected_index.borrow() {Style::default().fg(CATPPUCCIN.rosewater)} 
        // else {Style::default().fg(CATPPUCCIN.blue)};
        // let block = Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Rounded).border_style(style);

        self.selected_tab();

        let tab_order = self.tabs.len();
        let mut constraints = vec![Constraint::Length(20); tab_order];
        constraints.push(Constraint::Length(25)); // For the title paragraph

        let row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        for (idx, (_, button)) in self.tabs.iter().enumerate() {
            button.render_ref(row[idx].shrink(3, 1), f.buffer_mut());
        }
        // self.ticket_tab.render_ref(row[0].shrink(3, 1), f.buffer_mut());
        // self.scripts_tab.render_ref(row[1].shrink(3, 1), f.buffer_mut());
        // self.tasks_tab.render_ref(row[2].shrink(3, 1), f.buffer_mut());
        // self.system_tab.render_ref(row[3].shrink(3, 1), f.buffer_mut());
        // self.logs_tab.render_ref(row[4].shrink(3, 1), f.buffer_mut());
        // self.login_tab.render_ref(row[5].shrink(3, 1), f.buffer_mut());

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
            let client = get_client_hash();
            *title = client.connection_string.clone();
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
                .render_ref(row[7], f.buffer_mut());
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
        for (_, button) in self.tabs.iter() {
            button.handle_mouse_event(&mouse_event);
        }
    }
}
*/
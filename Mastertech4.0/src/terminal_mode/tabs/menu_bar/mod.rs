use crate::terminal_mode::{styling::CATPPUCCINTHEME, widgets::button::Button};
use crate::terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, styling::{CYAN, DARKORANGE, DEEPPINK, SPRINGGREEN}};
use std::{cell::RefCell, sync::{Arc, Mutex}};


pub mod action_handler;
pub mod tabs;
pub mod render;

use displays::app_state::AppState;
pub use tabs::Tab;

////////////////////////////////
/// MENU BAR
////////////////////////////////
// #[derive(Clone)]
pub struct MenuBar<'a> {
    pub current_tab: RefCell<Tab>,
    // pub tabs: HashMap<Tab, Button<'a>>,
    ticket_tab: Button<'a>,
    connect_ws_btn: Button<'a>,
    scripts_tab: Button<'a>,
    tasks_tab: Button<'a>,
    system_tab: Button<'a>,
    ncdu_tab: Button<'a>,
    webconsole_tab: Button<'a>,
    logs_tab: Button<'a>,
    pub login_tab: Button<'a>,
    ctx: Arc<Mutex<TerminalContext>>,
    client_title: String,
    connection_state: (bool, String),
    manual_start_tx: tokio::sync::mpsc::UnboundedSender<bool>
}

impl<'a> MenuBar<'a> {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>, manual_start_tx: tokio::sync::mpsc::UnboundedSender<bool>) -> Self {
        // Create tab buttons with .as_tab() to enable proper effect handling
        let mut ticket_tab = Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME).as_tab();
        ticket_tab.set_selected(true); // Default selected tab
        
        Self {
            ctx,
            connection_state: (false, String::new()),
            manual_start_tx,
            client_title: String::new(),
            current_tab: RefCell::new(Tab::TurSheet),
            ticket_tab,
            scripts_tab: Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN).as_tab(),
            system_tab: Button::new("System", WidgetId("System".to_owned())).theme(DEEPPINK).as_tab(),
            ncdu_tab: Button::new("NCDU", WidgetId("Ncdu".to_owned())).theme(DARKORANGE).as_tab(),
            webconsole_tab: Button::new("Webconsole", WidgetId("Webconsole".to_owned())).theme(CATPPUCCINTHEME).as_tab(),
            tasks_tab: Button::new("Tasks", WidgetId("Tasks".to_owned())).theme(CYAN).as_tab(),
            logs_tab: Button::new("Logs", WidgetId("Logs".to_owned())).theme(DEEPPINK).as_tab(),
            login_tab: Button::new("Login", WidgetId("Login".to_owned())).theme(DARKORANGE).as_tab(),
            connect_ws_btn: Button::new("Connect WS", WidgetId("Connect".to_owned())).theme(SPRINGGREEN),
        }
    }

    pub fn check_active_tab(&mut self) {
        if let Ok(mut ctx) = self.ctx.lock() {
            ctx.receive();
            match ctx.state {
                AppState::Authenticated(_) => {
                    if ctx.new_state {
                        ctx.new_state = false;
                        if let Ok(mut tab) = self.current_tab.try_borrow_mut() {
                            *tab = Tab::TurSheet;
                            self.login_tab.set_label("Logout".to_string());
                            // if let Ok(mut tab) = self.menu_bar.current_tab.try_borrow_mut() {
                            //     *tab = Tab::TurSheet;
                            //     if let Some(button) = self.menu_bar.tabs.get_mut(&tab) {
                            //         button.set_label("Logout".to_string());
                            //     }
                            // }
                        }
                    }
                },
                _ => {}
            }
        }
    }

    pub fn set_active_tab(&self, tab: Tab) {
        // Clear all tab selections
        self.ticket_tab.set_selected(false);
        self.scripts_tab.set_selected(false);
        self.system_tab.set_selected(false);
        self.ncdu_tab.set_selected(false);
        self.tasks_tab.set_selected(false);
        self.webconsole_tab.set_selected(false);
        self.logs_tab.set_selected(false);
        self.login_tab.set_selected(false);
        
        // Set the new tab as selected
        match tab {
            Tab::TurSheet => self.ticket_tab.set_selected(true),
            Tab::Scripts => self.scripts_tab.set_selected(true),
            Tab::SystemInfo => self.system_tab.set_selected(true),
            Tab::Ncdu => self.ncdu_tab.set_selected(true),
            Tab::Tasks => self.tasks_tab.set_selected(true),
            Tab::Webconsole => self.webconsole_tab.set_selected(true),
            Tab::Logs => self.logs_tab.set_selected(true),
            Tab::Login => self.login_tab.set_selected(true),
        }
        
        self.current_tab.replace(tab);
    }

    pub fn set_connection_state(&mut self, connection_state: (bool, String)) {
        self.connection_state = connection_state;
    }
}

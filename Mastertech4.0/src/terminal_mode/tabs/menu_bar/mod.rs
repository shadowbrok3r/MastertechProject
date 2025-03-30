use crate::terminal_mode::{styling::CATPPUCCINTHEME, widgets::button::Button};
use crate::terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, styling::{CYAN, DARKORANGE, DEEPPINK, SPRINGGREEN}};
use std::{cell::RefCell, sync::{Arc, Mutex}};


pub mod action_handler;
pub mod tabs;
pub mod render;

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
        // let mut tabs = HashMap::new();
        // tabs.insert(Tab::TurSheet, Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME));
        // tabs.insert(Tab::Scripts, Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN));
        // tabs.insert(Tab::SystemInfo, Button::new("System", WidgetId("System".to_owned())).theme(CATPPUCCINTHEME));
        // tabs.insert(Tab::Ncdu, Button::new("NCDU", WidgetId("Ncdu".to_owned())).theme(DEEPPINK));
        // tabs.insert(Tab::Tasks, Button::new("Tasks", WidgetId("Tasks".to_owned())).theme(DARKORANGE));
        // tabs.insert(Tab::Logs, Button::new("Logs", WidgetId("Logs".to_owned())).theme(CYAN));
        // tabs.insert(Tab::Login, Button::new("Login", WidgetId("Login".to_owned())).theme(SPRINGGREEN));

        Self {
            ctx,
            connection_state: (false, String::new()),
            manual_start_tx,
            // tabs,
            client_title: String::new(),
            // effect_stage: EffectStage::default(),
            current_tab: RefCell::new(Tab::TurSheet),
            ticket_tab: Button::new("Ticket", WidgetId("Ticket".to_owned())).theme(CATPPUCCINTHEME),
            scripts_tab: Button::new("Scripts", WidgetId("Scripts".to_owned())).theme(CYAN),
            system_tab: Button::new("System", WidgetId("System".to_owned())).theme(DEEPPINK),
            ncdu_tab: Button::new("NCDU", WidgetId("Ncdu".to_owned())).theme(DARKORANGE),
            webconsole_tab: Button::new("Webconsole", WidgetId("Webconsole".to_owned())).theme(CATPPUCCINTHEME),
            tasks_tab: Button::new("Tasks", WidgetId("Tasks".to_owned())).theme(CYAN),
            logs_tab: Button::new("Logs", WidgetId("Logs".to_owned())).theme(DEEPPINK),
            login_tab: Button::new("Login", WidgetId("Login".to_owned())).theme(DARKORANGE),
            connect_ws_btn: Button::new("Connect WS", WidgetId("Connect".to_owned())).theme(SPRINGGREEN),
        }
    }

    pub fn check_active_tab(&mut self) {
        if let Ok(mut ctx) = self.ctx.lock() {
            ctx.receive();
            match ctx.state {
                crate::app_state::AppState::Authenticated(_) => {
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
        self.current_tab.replace(tab);
    }

    pub fn set_connection_state(&mut self, connection_state: (bool, String)) {
        self.connection_state = connection_state;
    }
}

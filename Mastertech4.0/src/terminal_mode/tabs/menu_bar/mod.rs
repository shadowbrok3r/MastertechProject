use crate::terminal_mode::{
    context::TerminalContext,
    events::action_handler::{get_event_sender, WidgetButton, WidgetEvent, WidgetId},
    styling::ThemeRole,
    widgets::{
        button::{Button, ButtonState},
        dropdown_menu::DropdownMenu,
        menu_item::MenuItem,
        ButtonType,
    },
};
use database::schema::utilities::get_tasks_for_store;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    layout::Position,
};
use std::{cell::RefCell, sync::{Arc, Mutex}};

pub mod action_handler;
pub mod tabs;
pub mod render;

use displays::app_state::AppState;
pub use tabs::Tab;

/// The four top-level navigation groups. Each opens a hover dropdown of tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuGroup {
    Service,
    Tools,
    Remote,
    Account,
}

impl MenuGroup {
    pub const ALL: [MenuGroup; 4] = [
        MenuGroup::Service,
        MenuGroup::Tools,
        MenuGroup::Remote,
        MenuGroup::Account,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MenuGroup::Service => "Service",
            MenuGroup::Tools => "Tools",
            MenuGroup::Remote => "Remote",
            MenuGroup::Account => "Account",
        }
    }

    pub fn tabs(self) -> &'static [Tab] {
        match self {
            MenuGroup::Service => &[Tab::TurSheet, Tab::Tasks],
            MenuGroup::Tools => &[Tab::Scripts, Tab::SystemInfo, Tab::Ncdu, Tab::Assistant],
            MenuGroup::Remote => &[Tab::Webconsole, Tab::Logs],
            MenuGroup::Account => &[Tab::Login, Tab::Settings],
        }
    }

    pub fn of_tab(tab: Tab) -> MenuGroup {
        MenuGroup::ALL
            .into_iter()
            .find(|g| g.tabs().contains(&tab))
            .unwrap_or(MenuGroup::Service)
    }
}

/// Display label for a tab within a dropdown row.
fn tab_label(tab: Tab, login_label: &str) -> String {
    match tab {
        Tab::TurSheet => "Ticket".to_string(),
        Tab::Tasks => "Tasks".to_string(),
        Tab::Scripts => "Scripts".to_string(),
        Tab::SystemInfo => "System".to_string(),
        Tab::Ncdu => "NCDU".to_string(),
        Tab::Webconsole => "Webconsole".to_string(),
        Tab::Logs => "Logs".to_string(),
        Tab::Login => login_label.to_string(),
        Tab::Settings => "Theme".to_string(),
        Tab::Assistant => "Assistant".to_string(),
    }
}

////////////////////////////////
/// MENU BAR
////////////////////////////////
pub struct MenuBar<'a> {
    pub current_tab: RefCell<Tab>,
    service_trigger: Button<'a>,
    tools_trigger: Button<'a>,
    remote_trigger: Button<'a>,
    account_trigger: Button<'a>,
    connect_ws_btn: Button<'a>,
    /// "Login" until authenticated, then "Logout".
    login_label: RefCell<String>,
    dropdown: RefCell<DropdownMenu>,
    open_group: RefCell<Option<MenuGroup>>,
    ctx: Arc<Mutex<TerminalContext>>,
    client_title: String,
    connection_state: (bool, String),
    manual_start_tx: tokio::sync::mpsc::UnboundedSender<bool>,
}

impl<'a> MenuBar<'a> {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>, manual_start_tx: tokio::sync::mpsc::UnboundedSender<bool>) -> Self {
        let menu = Self {
            ctx,
            connection_state: (false, String::new()),
            manual_start_tx,
            client_title: String::new(),
            current_tab: RefCell::new(Tab::TurSheet),
            service_trigger: Button::new("Service", WidgetId("MenuService".to_owned())).menu_trigger(),
            tools_trigger: Button::new("Tools", WidgetId("MenuTools".to_owned())).menu_trigger(),
            remote_trigger: Button::new("Remote", WidgetId("MenuRemote".to_owned())).menu_trigger(),
            account_trigger: Button::new("Account", WidgetId("MenuAccount".to_owned())).menu_trigger(),
            connect_ws_btn: Button::new("Connect WS", WidgetId("Connect".to_owned())).theme(ThemeRole::Accent),
            login_label: RefCell::new("Login".to_string()),
            dropdown: RefCell::new(DropdownMenu::new()),
            open_group: RefCell::new(None),
        };
        menu.sync_active_group();
        menu
    }

    /// The four triggers paired with their group.
    fn triggers(&self) -> [(MenuGroup, &Button<'a>); 4] {
        [
            (MenuGroup::Service, &self.service_trigger),
            (MenuGroup::Tools, &self.tools_trigger),
            (MenuGroup::Remote, &self.remote_trigger),
            (MenuGroup::Account, &self.account_trigger),
        ]
    }

    fn trigger_for(&self, group: MenuGroup) -> &Button<'a> {
        match group {
            MenuGroup::Service => &self.service_trigger,
            MenuGroup::Tools => &self.tools_trigger,
            MenuGroup::Remote => &self.remote_trigger,
            MenuGroup::Account => &self.account_trigger,
        }
    }

    /// Marks the trigger whose group owns the current tab as selected (pink underline).
    fn sync_active_group(&self) {
        let active = MenuGroup::of_tab(*self.current_tab.borrow());
        for (group, btn) in self.triggers() {
            btn.set_selected(group == active);
        }
    }

    /// Builds the dropdown rows for a group, marking the current tab active.
    fn group_items(&self, group: MenuGroup) -> Vec<MenuItem> {
        let current = *self.current_tab.borrow();
        let login_label = self.login_label.borrow().clone();
        group
            .tabs()
            .iter()
            .map(|&tab| MenuItem::new(tab_label(tab, &login_label)).active(tab == current))
            .collect()
    }

    fn open_group(&self, group: MenuGroup) {
        let Some(anchor) = self.trigger_for(group).get_area() else {
            return;
        };
        let items = self.group_items(group);
        self.dropdown.borrow_mut().open_at(anchor, items, group.label());
        *self.open_group.borrow_mut() = Some(group);
        for (g, btn) in self.triggers() {
            btn.set_menu_open(g == group);
        }
    }

    fn close_menu(&self) {
        self.dropdown.borrow_mut().close();
        *self.open_group.borrow_mut() = None;
        for (_, btn) in self.triggers() {
            btn.set_menu_open(false);
        }
    }

    pub fn check_active_tab(&mut self) {
        if let Ok(mut ctx) = self.ctx.lock() {
            ctx.receive();
            if let AppState::Authenticated(_) = ctx.state {
                if ctx.new_state {
                    ctx.new_state = false;
                    *self.login_label.borrow_mut() = "Logout".to_string();
                    self.current_tab.replace(Tab::TurSheet);
                    self.sync_active_group();
                }
            }
        }
    }

    pub fn set_active_tab(&self, tab: Tab) {
        self.current_tab.replace(tab);
        self.sync_active_group();
    }

    /// Switches to `tab` and fires the per-tab side effects (System refresh,
    /// Tasks fetch, Webconsole side-panel toggle).
    fn activate_tab(&self, tab: Tab) {
        let current = *self.current_tab.borrow();
        match tab {
            Tab::SystemInfo => {
                let _ = get_event_sender().try_send(WidgetEvent::ButtonClick {
                    widget_id: WidgetId("RefreshSystemInfo".to_string()),
                    button: WidgetButton::Left,
                    source: Default::default(),
                });
            }
            Tab::Tasks => {
                if let Ok(ctx) = self.ctx.try_lock() {
                    let tx = ctx.tasks_tx.clone();
                    let store = ctx.user.get_store().as_str().to_string();
                    if !store.is_empty() {
                        tokio::spawn(async move {
                            let tasks_result = get_tasks_for_store(tx, store).await;
                            log::info!("Tasks result: {tasks_result:?}");
                        });
                    }
                }
            }
            Tab::Webconsole if current == Tab::Webconsole => {
                let _ = get_event_sender().try_send(WidgetEvent::ButtonClick {
                    widget_id: WidgetId("ToggleSidePanel".to_string()),
                    button: WidgetButton::Left,
                    source: Default::default(),
                });
                return;
            }
            _ => {}
        }
        self.set_active_tab(tab);
    }

    /// Mouse handling for the menu bar + dropdown. Returns true when the event
    /// was consumed (so the content tab below must not also act on it).
    pub fn handle_menu_mouse(&self, ev: &MouseEvent) -> bool {
        let pos = Position::new(ev.column, ev.row);
        self.connect_ws_btn.handle_mouse_event(ev);

        let open = *self.open_group.borrow();
        let hovered_trigger = self
            .triggers()
            .into_iter()
            .find(|(_, b)| b.get_area().map_or(false, |a| a.contains(pos)))
            .map(|(g, _)| g);

        match ev.kind {
            MouseEventKind::Moved => {
                for (g, btn) in self.triggers() {
                    btn.set_state(if Some(g) == hovered_trigger {
                        ButtonState::Hovered
                    } else {
                        ButtonState::Normal
                    });
                }
                if let Some(g) = hovered_trigger {
                    if open != Some(g) {
                        self.open_group(g);
                    }
                    return true;
                }
                if open.is_some() {
                    if self.dropdown.borrow().rect_contains(pos) {
                        self.dropdown.borrow_mut().on_mouse_move(pos);
                        return true;
                    } else if self.dropdown.borrow().bridge_contains(pos) {
                        return true;
                    } else {
                        self.close_menu();
                    }
                }
                false
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(g) = hovered_trigger {
                    if open == Some(g) {
                        self.close_menu();
                    } else {
                        self.open_group(g);
                    }
                    return true;
                }
                if let Some(group) = open {
                    let clicked = self.dropdown.borrow_mut().on_click(pos);
                    if let Some(idx) = clicked {
                        if let Some(&tab) = group.tabs().get(idx) {
                            self.activate_tab(tab);
                        }
                    }
                    self.close_menu();
                    return true;
                }
                false
            }
            _ => open.is_some(),
        }
    }

    /// Keyboard handling for an open dropdown. Returns true when consumed.
    pub fn handle_menu_key(&self, key: KeyEvent) -> bool {
        let open = *self.open_group.borrow();
        let Some(group) = open else {
            return false;
        };
        match key.code {
            KeyCode::Esc => {
                self.close_menu();
                true
            }
            KeyCode::Down => {
                self.dropdown.borrow_mut().select_next();
                true
            }
            KeyCode::Up => {
                self.dropdown.borrow_mut().select_prev();
                true
            }
            KeyCode::Enter => {
                let selected = self.dropdown.borrow().selected();
                if let Some(idx) = selected {
                    if let Some(&tab) = group.tabs().get(idx) {
                        self.activate_tab(tab);
                    }
                }
                self.close_menu();
                true
            }
            _ => false,
        }
    }

    pub fn set_connection_state(&mut self, connection_state: (bool, String)) {
        self.connection_state = connection_state;
    }
}

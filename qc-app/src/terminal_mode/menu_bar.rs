use mtech_tui::styling::THEME;
use mtech_tui::widgets::{
    button::{Button, ButtonState},
    dropdown_menu::DropdownMenu,
    menu_item::MenuItem,
    ButtonType,
};
use mtech_tui::events::action_handler::WidgetId;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Layout, Position, Rect},
    prelude::Backend,
    text::Line,
    widgets::Paragraph,
    Frame,
};
use std::cell::RefCell;

/// The eight QC views, mirroring the egui `QcTab` set 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    OrderQc,
    Stress,
    Hardware,
    SwiftDb,
    Oa3,
    Settings,
    Logs,
    BugReport,
    Ai,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Tab::OrderQc,
        Tab::Stress,
        Tab::Hardware,
        Tab::SwiftDb,
        Tab::Oa3,
        Tab::Settings,
        Tab::Logs,
        Tab::BugReport,
        Tab::Ai,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::OrderQc => "Order QC",
            Tab::Stress => "Stress",
            Tab::Hardware => "Hardware",
            Tab::SwiftDb => "Swift DB",
            Tab::Oa3 => "OA3 Sager",
            Tab::Settings => "Settings",
            Tab::Logs => "Logs",
            Tab::BugReport => "Bug Report",
            Tab::Ai => "Diagnose",
        }
    }
}

/// Top-level navigation groups; each opens a hover dropdown of tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuGroup {
    Order,
    Test,
    Tools,
    System,
}

impl MenuGroup {
    pub const ALL: [MenuGroup; 4] = [
        MenuGroup::Order,
        MenuGroup::Test,
        MenuGroup::Tools,
        MenuGroup::System,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MenuGroup::Order => "Order",
            MenuGroup::Test => "Test",
            MenuGroup::Tools => "Tools",
            MenuGroup::System => "System",
        }
    }

    pub fn tabs(self) -> &'static [Tab] {
        match self {
            MenuGroup::Order => &[Tab::OrderQc],
            MenuGroup::Test => &[Tab::Stress, Tab::Hardware],
            MenuGroup::Tools => &[Tab::SwiftDb, Tab::Oa3, Tab::Ai],
            MenuGroup::System => &[Tab::Settings, Tab::Logs, Tab::BugReport],
        }
    }

    pub fn of_tab(tab: Tab) -> MenuGroup {
        MenuGroup::ALL
            .into_iter()
            .find(|g| g.tabs().contains(&tab))
            .unwrap_or(MenuGroup::Order)
    }
}

pub struct MenuBar<'a> {
    current_tab: RefCell<Tab>,
    order_trigger: Button<'a>,
    test_trigger: Button<'a>,
    tools_trigger: Button<'a>,
    system_trigger: Button<'a>,
    dropdown: RefCell<DropdownMenu>,
    open_group: RefCell<Option<MenuGroup>>,
}

impl<'a> MenuBar<'a> {
    pub fn new() -> Self {
        let menu = Self {
            current_tab: RefCell::new(Tab::OrderQc),
            order_trigger: Button::new("Order", WidgetId("MenuOrder".to_owned())).menu_trigger(),
            test_trigger: Button::new("Test", WidgetId("MenuTest".to_owned())).menu_trigger(),
            tools_trigger: Button::new("Tools", WidgetId("MenuTools".to_owned())).menu_trigger(),
            system_trigger: Button::new("System", WidgetId("MenuSystem".to_owned())).menu_trigger(),
            dropdown: RefCell::new(DropdownMenu::new()),
            open_group: RefCell::new(None),
        };
        menu.sync_active_group();
        menu
    }

    pub fn current_tab(&self) -> Tab {
        *self.current_tab.borrow()
    }

    pub fn set_active_tab(&self, tab: Tab) {
        self.current_tab.replace(tab);
        self.sync_active_group();
    }

    /// Move `delta` steps through `Tab::ALL`, wrapping.
    pub fn cycle_tab(&self, delta: i32) {
        let cur = *self.current_tab.borrow();
        let n = Tab::ALL.len() as i32;
        let idx = Tab::ALL.iter().position(|t| *t == cur).unwrap_or(0) as i32;
        let next = ((idx + delta) % n + n) % n;
        self.set_active_tab(Tab::ALL[next as usize]);
    }

    fn triggers(&self) -> [(MenuGroup, &Button<'a>); 4] {
        [
            (MenuGroup::Order, &self.order_trigger),
            (MenuGroup::Test, &self.test_trigger),
            (MenuGroup::Tools, &self.tools_trigger),
            (MenuGroup::System, &self.system_trigger),
        ]
    }

    fn trigger_for(&self, group: MenuGroup) -> &Button<'a> {
        match group {
            MenuGroup::Order => &self.order_trigger,
            MenuGroup::Test => &self.test_trigger,
            MenuGroup::Tools => &self.tools_trigger,
            MenuGroup::System => &self.system_trigger,
        }
    }

    fn sync_active_group(&self) {
        let active = MenuGroup::of_tab(*self.current_tab.borrow());
        for (group, btn) in self.triggers() {
            btn.set_selected(group == active);
        }
    }

    fn group_items(&self, group: MenuGroup) -> Vec<MenuItem> {
        let current = *self.current_tab.borrow();
        group
            .tabs()
            .iter()
            .map(|&tab| MenuItem::new(tab.label()).active(tab == current))
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

    fn activate_tab(&self, tab: Tab) {
        self.set_active_tab(tab);
    }

    /// Mouse handling for the menu bar + dropdown. Returns true when consumed.
    pub fn handle_menu_mouse(&self, ev: &MouseEvent) -> bool {
        let pos = Position::new(ev.column, ev.row);
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
        let Some(group) = *self.open_group.borrow() else {
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
                if let Some(idx) = self.dropdown.borrow().selected() {
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

    /// Draw the trigger row + right-aligned title. Dropdown is painted by
    /// `draw_overlay` after the content tab so it lands on top.
    pub fn draw<B: Backend>(&self, f: &mut Frame, area: Rect) {
        let mut constraints: Vec<Constraint> = self
            .triggers()
            .iter()
            .map(|(g, _)| Constraint::Length(g.label().len() as u16 + 4))
            .collect();
        constraints.push(Constraint::Min(0));
        let cols = Layout::horizontal(constraints).split(area);
        for (i, (_, btn)) in self.triggers().iter().enumerate() {
            f.render_widget(*btn, cols[i]);
        }
        let title = Line::from(format!(" Mastertech QC - {} ", database::version_with_build!()))
            .right_aligned()
            .style(THEME.title());
        f.render_widget(Paragraph::new(title), cols[self.triggers().len()]);
    }

    pub fn draw_overlay(&self, f: &mut Frame) {
        if self.dropdown.borrow().is_open() {
            let area = f.area();
            self.dropdown.borrow_mut().render(f, area);
        }
    }
}

impl<'a> Default for MenuBar<'a> {
    fn default() -> Self {
        Self::new()
    }
}

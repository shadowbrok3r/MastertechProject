use crate::terminal_mode::context::TerminalContext;
use std::{cell::RefCell, sync::{Arc, Mutex}};
use database::schema::{LiveTaskPayload, User};
use tui_scrollview::ScrollViewState;
use ratatui::widgets::TableState;
use reqwest::Client;

pub mod action_handler;
pub mod render;
pub struct TasksTab {
    state: RefCell<TableState>,
    items: Vec<LiveTaskPayload>,
    widths: Vec<u16>, // Column widths
    scroll_state: RefCell<ScrollViewState>,
    pub _client: Client,
    ctx: Arc<Mutex<TerminalContext>>,
    _users: Vec<User>,
    current_user: User
}

impl TasksTab {
    pub fn new(_client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {

        Self {
            ctx,
            _client,
            state: RefCell::new(TableState::default()),
            items: Vec::new(),
            widths: Vec::new(),
            scroll_state: RefCell::new(ScrollViewState::default()),
            _users: Vec::new(),
            current_user: User::default(),
        }
    }

    pub fn check_tasks(&mut self) {
        
        if let Ok(mut ctx) = self.ctx.lock() {
            if ctx.new_tasks {
                ctx.new_tasks = false;
                for task in ctx.tasks.iter() {
                    if task.assignee == self.current_user.get_id() && !task.completed {
                        self.items.push(task.clone());
                    }
                }
        
                self.widths = Self::calculate_widths(&self.items);
            }
            if !ctx.user.get_name().is_empty() {
                self.current_user = ctx.user.clone();
            }
        }
    }   
}

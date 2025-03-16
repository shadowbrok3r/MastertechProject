use crate::terminal_mode::context::TerminalContext;
use std::sync::{Arc, Mutex};
use database::schema::TaskPayload;
use ratatui::widgets::{ScrollbarState, TableState};
use reqwest::Client;

pub mod action_handler;
pub mod render;

pub struct TasksTab {
    state: TableState,
    items: Vec<TaskPayload>,
    widths: Vec<u16>, // Column widths
    scroll_state: ScrollbarState,
    pub _client: Client,
    ctx: Arc<Mutex<TerminalContext>>,
}


impl TasksTab {
    pub fn new(_client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {

        Self {
            ctx,
            _client,
            state: TableState::default(),
            items: Vec::new(),
            widths: Vec::new(),
            scroll_state: ScrollbarState::default(),
        }
    }

    pub fn set_tasks(&mut self, tasks: Vec<TaskPayload>) {
        self.items = tasks;
    }   
}

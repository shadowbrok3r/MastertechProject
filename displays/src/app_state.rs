use std::collections::HashMap;

use crossbeam::channel::{Receiver, Sender};
use database::schema::{TaskPayload, User};
use serde::Serialize;

use crate::{tasks::task_layout::TaskLayout, TaskUiActions};




#[derive(Default, Debug, PartialEq)]
pub enum MainPages {
    #[default]
    Tasks,
    Downloads,
    WebConsole,
}

#[derive(Debug, PartialEq)]
pub enum AppState {
    Authenticated(MainPages),
    CreateAccount,
    NoAuth(String),
    Login,
}

impl Default for AppState {
    fn default() -> Self {
        Self::NoAuth("Not Authenticated".to_string())
    }
}

#[derive(Serialize)]
pub struct SharedContext {
    /// {Currently logged-in user}
    pub current_user: Option<User>,
    /// {Users in the store}
    pub store_users: Vec<User>,
    /// {Task layouts for different tabs}
    #[serde(skip)]
    pub task_layouts: HashMap<String, TaskLayout>,
    pub rerun_filtering_my_tasks: bool,
    pub rerun_filtering_store_tasks: bool,
    pub rerun_filtering_completed: bool,
    /// {All task data}
    pub tasks: Vec<TaskPayload>,
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,
    /// store selection for inventory view
    pub store_selection: u64,
}

impl Default for SharedContext {
    fn default() -> Self {
        let (ui_actions_tx, ui_actions_rx) = crossbeam::channel::unbounded::<TaskUiActions>();

        Self {
            current_user: None,
            tasks: Vec::new(),
            store_users: Vec::new(),
            ui_actions_tx,
            ui_actions_rx,
            task_layouts: HashMap::new(),
            rerun_filtering_my_tasks: false,
            rerun_filtering_store_tasks: false,
            rerun_filtering_completed: false,
            store_selection: 76
        }
    }
}
use crossbeam::channel::{Receiver, Sender};
use database::schema::{LiveTaskPayload, User};
use crate::AppState;
use super::{data::ServiceData, systems::communication_system::Message};

#[derive(Debug)]
pub struct TerminalContext {
    pub app_state_tx: Sender<AppState>,
    pub app_state_rx: Receiver<AppState>,
    pub url: Option<String>,
    pub state: AppState,
    pub new_state: bool,
    pub service_data: ServiceData,
    pub _render_sender: Sender<Box<dyn Message>>,
    pub data_sender: Sender<Box<dyn Message>>,
    pub user: User,
    pub store_users: Vec<User>,
    pub tasks: Vec<LiveTaskPayload>,
    pub tasks_tx: Sender<Vec<LiveTaskPayload>>,
    pub tasks_rx: Receiver<Vec<LiveTaskPayload>>,
    pub new_tasks: bool
    
}

impl TerminalContext {
    pub fn new(_render_sender: Sender<Box<dyn Message>>, data_sender: Sender<Box<dyn Message>>) -> Self {
        let (app_state_tx, app_state_rx) = crossbeam::channel::unbounded::<AppState>();
        let (tasks_tx, tasks_rx) = crossbeam::channel::unbounded::<Vec<LiveTaskPayload>>();
        Self {
            app_state_tx,
            app_state_rx,
            url: None,
            state: AppState::default(),
            service_data: ServiceData::new(),
            _render_sender,
            data_sender,
            new_state: false,
            user: User::default(),
            store_users: vec![],
            tasks: Vec::new(),
            tasks_tx,
            tasks_rx,
            new_tasks: false,
        }
    }
}

impl TerminalContext {
    pub fn receive(&mut self) {
        self.service_data.receive_computer_data();
        
        if let Ok(tasks) = self.tasks_rx.try_recv() {
            self.new_tasks = true;
            self.tasks = tasks;
        }

        if let Ok(state) = self.app_state_rx.try_recv() {
            self.new_state = true;
            self.state = state;
        }
    }
}
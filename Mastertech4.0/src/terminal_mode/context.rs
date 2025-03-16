use crossbeam::channel::{Receiver, Sender};
use database::schema::{TaskPayload, User, CONNECTED_CLIENT_TABLE};
use surrealdb::RecordId;
use uuid::Uuid;

use crate::AppState;

use super::{data::ServiceData, systems::communication_system::Message};



#[derive(Debug)]
pub struct TerminalContext {
    pub client_uuid: RecordId,
    pub app_state_tx: Sender<AppState>,
    pub app_state_rx: Receiver<AppState>,
    pub url: Option<String>,
    pub client_title: String,
    pub state: AppState,
    pub new_state: bool,
    pub service_data: ServiceData,
    pub render_sender: Sender<Box<dyn Message>>,
    pub data_sender: Sender<Box<dyn Message>>,
    pub user: User,
    pub tasks: Vec<TaskPayload>,
}

impl TerminalContext {
    pub fn new(render_sender: Sender<Box<dyn Message>>, data_sender: Sender<Box<dyn Message>>) -> Self {
        let (app_state_tx, app_state_rx) = crossbeam::channel::unbounded::<AppState>();
        let client_uuid = RecordId::from((CONNECTED_CLIENT_TABLE, Uuid::new_v4().to_string()));
        
        Self {
            client_uuid,
            app_state_tx,
            app_state_rx,
            url: None,
            client_title: String::new(),
            state: AppState::default(),
            service_data: ServiceData::default(),
            render_sender,
            data_sender,
            new_state: false,
            user: User::default(),
            tasks: Vec::new(),
        }
    }
}

impl TerminalContext {
    pub fn receive(&mut self) {
        if let Ok(state) = self.app_state_rx.try_recv() {
            self.new_state = true;
            self.state = state;
        }
    }
}
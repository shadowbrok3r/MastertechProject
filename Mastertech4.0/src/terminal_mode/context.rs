use crossbeam::channel::{Receiver, Sender};
use database::schema::CONNECTED_CLIENT_TABLE;
use surrealdb::RecordId;
use uuid::Uuid;

use crate::AppState;

use super::{data::ServiceData, systems::communication_system::Message};



#[derive(Debug)]
pub struct TerminalContext {
    pub client_uuid: RecordId,
    // pub current_user: Option<User>,
    // pub disks: Value,
    // pub disk_num: usize,
    // pub db_rx: Receiver<anyhow::Result<Database, anyhow::Error>>,
    // pub db_tx: Sender<anyhow::Result<Database, anyhow::Error>>,
    pub app_state_tx: Sender<AppState>,
    pub app_state_rx: Receiver<AppState>,
    pub url: Option<String>,
    // pub client_friendly_name: String,
    pub client_title: String,
    pub state: AppState,
    pub new_state: bool,
    pub service_data: ServiceData,
    pub render_sender: Sender<Box<dyn Message>>,
    pub data_sender: Sender<Box<dyn Message>>,
}

impl TerminalContext {
    pub fn new(render_sender: Sender<Box<dyn Message>>, data_sender: Sender<Box<dyn Message>>) -> Self {
        // let (db_tx, db_rx) = crossbeam::channel::unbounded();
        let (app_state_tx, app_state_rx) = crossbeam::channel::unbounded::<AppState>();
        let client_uuid = RecordId::from((CONNECTED_CLIENT_TABLE, Uuid::new_v4().to_string()));
        
        Self {
            client_uuid,
            // db_rx, db_tx,
            app_state_tx,
            app_state_rx,
            url: None,
            // client_friendly_name: String::new(),
            client_title: String::new(),
            // current_user: None,
            state: AppState::default(),
            service_data: ServiceData::default(),
            render_sender,
            data_sender,
            new_state: false,
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
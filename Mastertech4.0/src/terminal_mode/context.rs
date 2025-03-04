use crossbeam::channel::{Receiver, Sender};
use database::{schema::{User, CONNECTED_CLIENT_TABLE}, Database};
use surrealdb::RecordId;
use uuid::Uuid;

use crate::{AppState, MainPages};



pub struct TerminalContext {
    pub client_uuid: RecordId,
    pub current_user: Option<User>,
    // pub disks: Value,
    // pub disk_num: usize,
    // pub db_rx: Receiver<anyhow::Result<Database, anyhow::Error>>,
    // pub db_tx: Sender<anyhow::Result<Database, anyhow::Error>>,
    pub app_state_tx: Sender<AppState>,
    pub app_state_rx: Receiver<AppState>,
    pub url: Option<String>,
    // pub client_friendly_name: String,
    pub client_title: String,
    pub state: AppState
}

impl Default for TerminalContext {
    fn default() -> Self {
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
            current_user: None,
            state: AppState::default()
        }
    }
}

impl TerminalContext {
    pub fn receive(&mut self) {
        if let Ok(state) = self.app_state_rx.try_recv() {
            self.state = state;
        }
        
        // if let Ok(db) = self.db_rx.try_recv() {
        //     match db {
        //         Ok(db) => {
        //             log::info!("3");
        //             if self.current_user.is_none() && db.user.is_some() {
        //                 log::info!("10");
        //                 self.current_user = db.user;
        //             } else {
        //                 log::info!("11");
        //             }
        //             let _ = self.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
        //         }
        //         Err(e) => {
        //             log::info!("6");
        //             if e.to_string().contains("Already connected") {
        //                 log::info!("7");
        //                 let _ = self.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
        //             } else {
        //                 log::info!("8");
        //                 log::info!("{e:?}");
        //                 let _ = self.app_state_tx.try_send(AppState::NoAuth("Needs login".to_string()));
        //             }
        //         }
        //     }
        // }
    }
}
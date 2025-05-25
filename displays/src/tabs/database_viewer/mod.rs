use crossbeam::channel::{unbounded, Receiver, Sender};
use row_viewer::{DatabaseRowViewer, DatabaseTable, DatabaseTableSelection};
use egui_data_table::DataTable;
use std::collections::HashMap;

// pub mod tables;
pub mod data;
// pub mod codec;
pub mod row_viewer;
pub mod ui;

pub struct DatabaseViewer {
    pub database_viewer: DatabaseRowViewer,
    pub table_map: HashMap<String, DataTable<DatabaseTable>>,
    pub data_selection_tx: Sender<DatabaseTableSelection>,
    pub data_selection_rx: Receiver<DatabaseTableSelection>,
    pub data_tx: Sender<DatabaseTable>,
    pub data_rx: Receiver<DatabaseTable>,
    pub start_idx: i32,
}

impl Default for DatabaseViewer {
    fn default() -> Self {
        let (data_selection_tx, data_selection_rx) = unbounded();
        let (data_tx, data_rx) = unbounded();
        Self { 
            database_viewer: Default::default(), 
            table_map: Default::default(), 
            data_selection_tx, data_selection_rx,
            data_tx, data_rx,
            start_idx: 0,
        }
    }
}



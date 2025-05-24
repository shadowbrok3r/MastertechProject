use row_viewer::{DatabaseRowViewer, DatabaseTable, DatabaseTableSelection};
use egui_data_table::DataTable;
use std::collections::HashMap;

// pub mod tables;
// pub mod data;
// pub mod codec;
pub mod row_viewer;
pub mod ui;

#[derive(Default)]
pub struct DatabaseViewer {
    pub database_viewer: DatabaseRowViewer,
    pub selected_table: DatabaseTableSelection,
    pub table_map: HashMap<String, DataTable<DatabaseTable>>,
}



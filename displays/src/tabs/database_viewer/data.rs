use database::schema::{LiveTaskPayload, TaskPayload, User};
use itertools::Itertools;

use crate::{get_database_users, PlatformSpawner, Spawner};

use super::{row_viewer::DatabaseRowViewer, DatabaseEditor};

impl DatabaseEditor {
    pub fn receive(&mut self) {
        if let Ok(data) = self.data_selection_rx.try_recv() {
            let tx = self.data_tx.clone();
            let start_idx = self.start_idx.clone();
            let users = &self.database_viewer.store_users;
            if users.is_empty() {
                self.database_viewer.store_users = get_database_users();
            }
            PlatformSpawner::spawn(async move {
                match data {
                    super::row_viewer::DatabaseTableSelection::Task => {
                                        let tasks = LiveTaskPayload::get_tasks(start_idx).await.unwrap_or_default();
                                        log::info!("{}", tasks.len());
                                        for task in tasks.iter() {
                                            let _ = tx.try_send(super::row_viewer::DatabaseTable::Task(TaskPayload::from(task.clone())));
                                        }
                                    },
                    super::row_viewer::DatabaseTableSelection::User => {
                                        let users = User::get_users().await.unwrap_or_default();
                                        for user in users.iter() {
                                            let _ = tx.try_send(super::row_viewer::DatabaseTable::User(user.clone()));
                                        }
                                    },
                    super::row_viewer::DatabaseTableSelection::Service => todo!(),
                    super::row_viewer::DatabaseTableSelection::Customer => todo!(),
                    super::row_viewer::DatabaseTableSelection::Computer => todo!(),
                }
            });
        }

        if let Ok(data) = self.data_rx.try_recv() {
            let key = self.database_viewer.selected_table.as_str();
            self.database_viewer.selected = data.clone();
            self.table_map
                .entry(key.to_string())
                .or_insert(egui_data_table::DataTable::new());

            if let Some(k) = self.table_map.get_mut(&key.to_string()) {
                if !k.iter().contains(&data) {
                    k.push(data);
                }
            }
        }
    }
}

impl DatabaseRowViewer {

}

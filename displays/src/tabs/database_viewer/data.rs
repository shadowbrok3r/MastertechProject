use database::schema::{ComputerData, CustomerData, DiagnosticEntry, DiagnosticSession, LiveTaskPayload, PluginRegistryEntry, TicketPayload, User};
use crate::{get_database_users, PlatformSpawner, Spawner};
use itertools::Itertools;
use super::DatabaseEditor;

impl DatabaseEditor {
    pub fn receive(&mut self, ctx: &eframe::egui::Context) {
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
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::Task(task.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::User => {
                        let users = User::get_users().await.unwrap_or_default();
                        for user in users.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::User(user.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::Service => {
                        let tickets = TicketPayload::get_services(start_idx).await.unwrap_or_default();
                        for ticket in tickets.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::Service(ticket.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::Customer => {
                        let customers = CustomerData::get_customers(start_idx).await.unwrap_or_default();
                        for customer in customers.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::Customer(customer.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::Computer => {
                        let computers = ComputerData::get_computers(start_idx).await.unwrap_or_default();
                        for computer in computers.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::Computer(computer.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::DiagSession => {
                        let sessions = DiagnosticSession::list_all(start_idx).await.unwrap_or_default();
                        for s in sessions.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::DiagSession(s.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::DiagEntry => {
                        let entries = DiagnosticEntry::list_all(start_idx).await.unwrap_or_default();
                        for e in entries.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::DiagEntry(e.clone()));
                        }
                    },
                    super::row_viewer::DatabaseTableSelection::PluginReg => {
                        let plugins = PluginRegistryEntry::list_all().await.unwrap_or_default();
                        for p in plugins.iter() {
                            let _ = tx.try_send(super::row_viewer::DatabaseTable::PluginReg(p.clone()));
                        }
                    },
                }
            });
        }

        let mut got_row = false;
        while let Ok(data) = self.data_rx.try_recv() {
            got_row = true;
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
        if got_row {
            ctx.request_repaint();
        }
    }
}

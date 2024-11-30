// use database::{schema::buckets::list_buckets, STORAGE_URL};
use eframe::egui::Context;
use log::{error, info};
// use tokio::spawn;

use crate::app_state::{AppState, MasterTechApp};

impl MasterTechApp {
    pub fn receive_database(&mut self, ctx: &Context) {
        if let Ok(db) = self.context.db_rx.try_recv() {
            info!("Received DB connection from thread");
            self.context.first_run = true;
            match db {
                Ok(db) => {
                    self.context.shared_ctx.current_user = db.user.clone();
                    if let Some(_usr) = db.user {
                        self.load_data(ctx);
                        // if let (Some(access_key), Some(secret_key)) =
                        //     (usr.minio_access_key.clone(), usr.minio_secret_key.clone())
                        // {
                        //     self.context.toolbox.access_key = access_key.clone();
                        //     self.context.toolbox.secret_key = secret_key.clone();
                        //     self.context.toolbox.set_user(usr.clone());
                        //     let minio_tx = self.context.minio_files.0.clone();
                        //     let name = usr.email.clone();
                        //     let parsed = name
                        //         .split_once('@')
                        //         .unwrap_or_default()
                        //         .0
                        //         .to_string()
                        //         .clone();

                        //     info!("Getting Minio files");

                        //     spawn(async move {
                        //         let list_bucket_res = list_buckets(
                        //             STORAGE_URL.to_string(),
                        //             access_key,
                        //             secret_key,
                        //             parsed,
                        //         )
                        //         .await;

                        //         match list_bucket_res {
                        //             Ok(files) => {
                        //                 info!("Got files: {files:?}");
                        //                 minio_tx.try_send(files).unwrap()
                        //             }
                        //             Err(e) => error!("Error getting minio files: {e:?}"),
                        //         }
                        //     });
                        // }
                    }
                }
                Err(e) => {
                    error!("Error with auth: {e:?}");
                    self.state = AppState::NoAuth(e.to_string());
                    self.context.shared_ctx.current_user = None;
                }
            }
        }
    }
}

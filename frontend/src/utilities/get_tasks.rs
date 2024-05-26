
use futures::StreamExt;
use database::{schema::*, Database};
use serde::{Deserialize, Serialize};
use surrealdb::{method::Stream, Notification};
use wasm_bindgen_futures::spawn_local;
use std::{marker, fmt::Debug};
use log::{info, error};
use crossbeam::channel::Sender;
use surrealdb::engine::remote::ws::Client;
use serde::de::DeserializeOwned;
use egui::{Ui, Response};
use crate::utilities::task_context::TaskContext;
use my_proc_macros::DelegateTraits;

use super::{Displayable, Updatable, FilterTasks, Interaction};


pub fn get_my_tasks(db: Database, tx: Sender<Vec<TaskPayload>>, initials: String)
{
    spawn_local(async move {
        let query = format!(
            "SELECT * FROM task 
            WHERE assignee_initials == '{initials}' "
        );
        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => info!("Sent Data from querying tasks"),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }

    });
}

pub fn get_store_tasks(db: Database, tx: Sender<Vec<TaskPayload>>, store: Store)
{
    spawn_local(async move {
        let query = format!(
            "SELECT * FROM task \
            WHERE dep == '{store:?}'"
        );
        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        info!("get_store_tasks: {query_results:?}");
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => info!("Sent Data from querying tasks"),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }
    });
}

pub fn get_completed_tasks(db: Database, tx: Sender<Vec<TaskPayload>>, store: Store)
{
    spawn_local(async move {
        let query = format!(
            "SELECT * FROM task \
            WHERE dep == '{store:?}' && task.completed == true"
        );
        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        info!("get_completed_tasks: {query_results:?}");
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => info!("Sent Data from querying tasks"),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }
    });
}

pub fn listen_tasks<T>(db: Database, tx: Sender<T>) 
    where T: DeserializeOwned + Serialize + 'static + Debug + marker::Unpin
{
    spawn_local(async move {
        let task_stream: Stream<Client, Vec<T>> = db.database.select(TASK_TABLE).live().await.unwrap();
        handle_streams(task_stream, tx.clone()).await;
    });
}



async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    tx: Sender<T>
) where T: Serialize + Deserialize<'static> + Debug{
    while let Some(notification) = notification_stream.next().await {
        match notification{
            Ok(notification) => {
                // let action = notification.action;
                let data = notification.data;
                let action = format!("{:?}", notification.action);
                info!("{action}\n{data:?}");
                match tx.send(data){
                    Ok(_) => info!("Sending task data over channel"),
                    Err(e) => error!("Error sending task data: {e:?}")
                }
            },
            Err(err) => {
                error!("Error: {err:?}");
            }
        };
    }; 
}

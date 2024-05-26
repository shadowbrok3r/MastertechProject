use database::schema::TaskPayload;
use futures::StreamExt;
use database::{schema::*, Database};
use serde::{Deserialize, Serialize};
use surrealdb::{method::Stream, Action, Notification};
use wasm_bindgen_futures::spawn_local;
use std::{marker, fmt::Debug};
use log::{info, error};
use crossbeam::channel::Sender;
use surrealdb::engine::remote::ws::Client;
use serde::de::DeserializeOwned;
use super::LiveUpdate;


pub fn handle_live_data(data: (Action, TaskPayload), existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>{
    match data.0{
        Action::Create => {
            data.1.handle_live_create(existing_tasks)?;
        },
        Action::Update => {
            data.1.handle_live_delete(existing_tasks)?;
        },
        Action::Delete => {
            data.1.handle_live_update(existing_tasks)?;
        },
        _ => {},
    }
    Ok(())
}

impl LiveUpdate for TaskPayload {
    fn handle_live_create(self, _existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Created: {:?}", self);
        Ok(())
    }
    
    fn handle_live_update(self, _existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Updated: {:?}", self);
        Ok(())
    }
    
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Deleted: {:?}", self);
        update_or_insert(existing_tasks, self);
        Ok(())
    }
}


pub fn update_or_insert(tasks: &mut Vec<TaskPayload>, new_task: TaskPayload) {
    if let Some(ref id) = new_task.id {
        
        let mut updated = false;

        for task in tasks.iter_mut() {
            if let Some(existing_id) = &task.id {
                if existing_id == id{
                    info!("ID's match: {:?} // {:?}", existing_id, id);
                    *task = new_task.clone();
                    updated = true;
                    break;
                }
            }
        }

        if !updated {
            // Insert the new task if it does not exist
            tasks.push(new_task);
        }
    } else {
        // If the new task does not have an ID, insert it
        tasks.push(new_task);
    }
}


pub fn listen_tasks<T>(db: Database, tx: Sender<(Action, T)>) 
    where T: DeserializeOwned + Serialize + 'static + Debug + marker::Unpin
{
    spawn_local(async move {
        let task_stream: Stream<Client, Vec<T>> = db.database.select(TASK_TABLE).live().await.unwrap();
        handle_streams(task_stream, tx).await;
    });
}



async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    tx: Sender<(Action, T)>
) where T: Serialize + Deserialize<'static> + Debug{
    while let Some(notification) = notification_stream.next().await {
        match notification{
            Ok(notification) => {
                let data = notification.data;
                // info!("{action}\n{data:?}");
                match tx.send((notification.action, data)){
                    Ok(_) => {
                        info!("Sent notification");
                        // drop(tx);
                        // break;
                    },
                    Err(e) => {
                        error!("Error sending task data: {e:?}");
                        // break;
                    }
                }
            },
            Err(err) => {
                error!("Error: {err:?}");
                // break;
            }
        };
    }; 
}

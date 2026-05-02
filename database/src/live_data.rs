use super::{DATABASE, schema::{utilities::LiveUpdate, ConnectedClient, LiveTaskPayload, RecordIdExt, TaskNotePayload, TaskPayload}};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use crossbeam::channel::Sender;
use log::{debug, error, info};
use structdiff::StructDiff;
use futures::StreamExt;
use std::fmt::Debug;
use surrealdb::types::SurrealValue;

// Re-export or define Action enum for live query actions
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Create,
    Update,
    Delete,
}

pub fn handle_live_data((action, data): (Action, LiveTaskPayload), existing_tasks: &mut Vec<LiveTaskPayload>) 
    -> anyhow::Result<(), anyhow::Error>
{
    match action{
        Action::Create => {
            data.handle_live_create(existing_tasks)?;
        },
        Action::Update => {
            data.handle_live_update(existing_tasks)?;
        },
        Action::Delete => {
            data.handle_live_delete(existing_tasks)?;
        },
    }
    Ok(())
}

pub fn handle_live_notes((action, data): (Action, TaskNotePayload), existing_task: &mut TaskPayload) 
    -> anyhow::Result<(), anyhow::Error>
{
    match action{
        Action::Create | Action::Update => {
            update_or_insert_notes(data, existing_task)?;
        },
        Action::Delete => {
            debug!("Data: {data:?}");
            let notes = &mut existing_task.task_note;
            if !notes.is_empty(){
                let index = notes.iter().position(|x| *x == data);
                if let Some(idx) = index {
                    notes.remove(idx);
                }
            }
            // update_or_insert_notes(data, existing_task)?;
        },
    }
    Ok(())
}

pub fn handle_live_create<T: Serialize + for<'a> Deserialize<'a> + Debug>(existing_data: &mut Vec<T>, new_data: T) -> anyhow::Result<(), anyhow::Error> {
    debug!("Data was Created: {:?}", new_data);
    existing_data.push(new_data);
    Ok(())
}

pub fn handle_live_update<T: Serialize + for<'a> Deserialize<'a> + Debug + PartialEq>(existing_data: &mut Vec<T>, new_data: T) -> anyhow::Result<(), anyhow::Error> {
    debug!("Data was Updated: {:?}", new_data);
    let index = existing_data.iter().position(|x| *x == new_data);
    if let Some(idx) = index {
        if let Some(dat) = existing_data.get_mut(idx) {
            info!("Replacing existing_data@{idx} with -> {:?}", dat);
            *dat = new_data;
        }
    }
    Ok(())
}

pub fn handle_live_delete<T: Serialize + for<'a> Deserialize<'a> + Debug + PartialEq>(existing_data: &mut Vec<T>, data_to_delete: T) -> anyhow::Result<(), anyhow::Error> {
    debug!("Data was Deleted: {:?}", data_to_delete);
    let index = existing_data.iter().position(|x| *x == data_to_delete);
    if let Some(idx) = index {
        info!("Deleting @ {idx}");
        existing_data.remove(idx);
    }
    Ok(())
}

/// Delete a ConnectedClient by comparing connection_string (unique identifier)
pub fn handle_live_delete_client(existing_clients: &mut Vec<ConnectedClient>, client_to_delete: ConnectedClient) -> anyhow::Result<(), anyhow::Error> {
    debug!("Client was Deleted: {:?}", client_to_delete);
    let index = existing_clients.iter().position(|x| x.connection_string == client_to_delete.connection_string);
    if let Some(idx) = index {
        info!("Deleting client @ {idx}");
        existing_clients.remove(idx);
    }
    Ok(())
}

impl LiveUpdate for LiveTaskPayload {
    fn handle_live_create(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Created: {:?}", self);
        update_or_insert(existing_tasks, self)?;
        Ok(())
    }
    
    fn handle_live_update(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Updated: {:?}", self);
        update_or_insert(existing_tasks, self)?;
        Ok(())
    }
    
    fn handle_live_delete(self, existing_tasks: &mut Vec<LiveTaskPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Deleted: {:?}", self);
        if !existing_tasks.is_empty(){
            let index = existing_tasks.iter().position(|x| x.id == self.id);
            info!("Index of deleted data: {index:?}");
            if let Some(idx) = index {
                existing_tasks.remove(idx);
            }
        }
        Ok(())
    }
}

pub fn update_or_insert_notes(new_note: TaskNotePayload, task: &mut TaskPayload) -> anyhow::Result<(), anyhow::Error> {
    if let Some(ref task_id) = new_note.task_id {
        let existing_task_id = &task.id;
        if existing_task_id == task_id {
            let notes = &mut task.task_note;
            
            if let Some(existing_note) = notes.iter_mut().find(|note| {
                note.id.key_string() == new_note.id.key_string()
            }) {
                // Apply diffs to the existing note
                let diffs = existing_note.diff(&new_note);
                existing_note.apply_mut(diffs);
                debug!("Updated existing note: {:?}", existing_note);
            } else {
                // Insert the new note if it doesn't exist
                notes.push(new_note.clone());
                debug!("Inserted new note: {:?}", new_note);
            }
        }
    }
    Ok(())
}

pub fn update_or_insert_anything<T: StructDiff + PartialEq + Debug>(current_data: &mut Vec<T>, new_data: T) 
    -> anyhow::Result<(), anyhow::Error>
{
    let mut updated = false;
    for existing_data in &mut current_data.iter_mut() {
        if *existing_data == new_data{
            info!("existing_data match's new_data: {:?} // {:?}", existing_data, new_data);
            let diffs = existing_data.diff(&new_data);
            existing_data.apply_mut(diffs);
            updated = true;
            break;
        } 
    }
    if !updated {
        current_data.push(new_data);
    }
    Ok(())
}

/// Update or insert a ConnectedClient by comparing connection_string (unique identifier)
/// This is needed because ConnectedClient's PartialEq compares all fields, 
/// causing false negatives when last_update differs
pub fn update_or_insert_client(current_clients: &mut Vec<ConnectedClient>, new_client: ConnectedClient) 
    -> anyhow::Result<(), anyhow::Error>
{
    let mut updated = false;
    for existing_client in current_clients.iter_mut() {
        // Compare by connection_string which is the unique identifier
        if existing_client.connection_string == new_client.connection_string {
            debug!("Updating existing client: {}", new_client.connection_string);
            let diffs = existing_client.diff(&new_client);
            existing_client.apply_mut(diffs);
            updated = true;
            break;
        } 
    }
    if !updated {
        debug!("Inserting new client: {}", new_client.connection_string);
        current_clients.push(new_client);
    }
    Ok(())
}

pub fn update_or_insert(tasks: &mut Vec<LiveTaskPayload>, new_task: LiveTaskPayload) 
    -> anyhow::Result<(), anyhow::Error>
{
    let mut updated = false;

    for existing_task in tasks.iter_mut() {
        if existing_task.id.clone() == new_task.id {
            info!("ID's match: {:?} // {:?}", &existing_task.id, &new_task.id);
            // Calculate the diff and apply it to the existing task
            let diffs = existing_task.diff(&new_task.clone());
            existing_task.apply_mut(diffs);

            *existing_task = new_task.clone();
            updated = true;
            break;
        }
    }
    
    if !updated {
        info!("data was NOT updated");
        tasks.push(new_task.into());
    }
    
    Ok(())
}

pub fn update_or_insert_layout(
    tasks: &mut Vec<LiveTaskPayload>, 
    new_task: LiveTaskPayload,
    task_to_replace: &mut LiveTaskPayload
) 
    -> anyhow::Result<(), anyhow::Error> 
{
    let id = &new_task.id;
    let mut updated = false;
    for task in tasks.iter_mut() {
        let existing_id = &task.id;
        if existing_id == id {
            debug!("ID's match: {:?} // {:?}", existing_id, id);
            // Calculate the diff and apply it to the existing task
            let diffs = task.diff(&new_task);
            task.apply_mut(diffs);

            // Also update the task_to_replace with the updated task
            *task_to_replace = task.clone();
            updated = true;
            break;
        }
    }

    if !updated {
        debug!("Data was NOT updated; inserting new task.");
        tasks.push(new_task.into());
    }
    Ok(())
}


// In SurrealDB 3.0, live queries are handled differently
// The new API uses a different streaming approach
pub async fn listen_data<T: DeserializeOwned + Serialize + 'static + Debug + std::marker::Unpin + SurrealValue>(tx: Sender<(Action, T)>, resource: &str) 
    -> anyhow::Result<(), anyhow::Error> 
{
    let mut data_stream = DATABASE.select(resource).live().await?;
    
    while let Some(notification) = data_stream.next().await {
        match notification {
            Ok(notif) => {
                let data = notif.data;
                let action = match notif.action {
                    surrealdb_types::Action::Create => Action::Create,
                    surrealdb_types::Action::Update => Action::Update,
                    surrealdb_types::Action::Delete => Action::Delete,
                    surrealdb_types::Action::Killed => {
                        error!("Live query was killed");
                        return Err(anyhow::anyhow!("Live query was killed"));
                    },
                    surrealdb_types::Action::Error => {
                        error!("Live query received an error action");
                        return Err(anyhow::anyhow!("Live query received an error action"));
                    },
                    _ => continue,
                };
                debug!("Data: {:?}", action);
                if let Err(e) = tx.try_send((action, data)) {
                    error!("Error Sending notification {e:?}");
                }
            },
            Err(e) => {
                error!("Error in notification stream: {e:?}");
                return Err(anyhow::anyhow!("I/O error: {}", e));
            },
        }
    }
    Ok(())
}

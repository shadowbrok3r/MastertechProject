use super::{DATABASE, schema::{utilities::LiveUpdate, LiveTaskPayload, TaskNotePayload, TicketPayload, TaskPayload}};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use surrealdb::{method::Stream, Action, Notification};
use std::fmt::Debug;
use structdiff::StructDiff;
use crossbeam::channel::Sender;
use futures::StreamExt;
use log::{debug, error, info};

use anyhow::Error;

pub fn handle_live_data((action, data): (Action, LiveTaskPayload), existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) 
    -> anyhow::Result<(), anyhow::Error>
{
    match action{
        Action::Create => {
            data.handle_live_create(existing_tasks, new_ticket)?;
        },
        Action::Update => {
            data.handle_live_update(existing_tasks, new_ticket)?;
        },
        Action::Delete => {
            data.handle_live_delete(existing_tasks, new_ticket)?;
        },
        _ => {},
    }
    Ok(())
}

pub fn handle_live_notes((action, data): (Action, TaskNotePayload), existing_task: &mut TaskPayload) 
    -> anyhow::Result<(), anyhow::Error>
{
    match action{
        Action::Create => {
            update_or_insert_notes(data, existing_task)?;
        },
        Action::Update => {
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
        _ => {},
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

impl LiveUpdate for LiveTaskPayload {
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Created: {:?}", self);
        update_or_insert(existing_tasks, self, new_ticket)?;
        Ok(())
    }
    
    fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Updated: {:?}", self);
        update_or_insert(existing_tasks, self, new_ticket)?;
        Ok(())
    }
    
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>, _new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Deleted: {:?}", self);
        let data = self.clone();
        if !existing_tasks.is_empty(){
            let index = existing_tasks.iter().position(|x| *x == data.clone().into());
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
                note.id.key().to_string() == new_note.id.key().to_string()
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

pub fn update_or_insert(tasks: &mut Vec<TaskPayload>, new_task: LiveTaskPayload, new_ticket: Option<TicketPayload>) 
    -> anyhow::Result<(), anyhow::Error>
{
    let id = &new_task.id;
    let mut updated = false;

    for task in tasks.iter_mut() {
        let existing_id = &task.id;
        if existing_id == id{
            info!("ID's match: {:?} // {:?}", existing_id, id);
            let mut updated_task: TaskPayload = new_task.clone().into(); // convert_live_to_task(new_task.clone(), task, new_ticket);

            updated_task.service_ticket = if let Some(service) = new_ticket {
                Some(service)
            } else { task.service_ticket.clone() };
            updated_task.task_note = task.task_note.clone();
            // Calculate the diff and apply it to the existing task
            let diffs = task.diff(&updated_task);
            task.apply_mut(diffs);

            *task = updated_task;
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
    tasks: &mut Vec<TaskPayload>, 
    new_task: LiveTaskPayload,
    new_ticket: Option<TicketPayload>,
    task_to_replace: &mut TaskPayload
) 
    -> anyhow::Result<(), anyhow::Error> 
{
    let id = &new_task.id;
    let mut updated = false;
    for task in tasks.iter_mut() {
        let existing_id = &task.id;
        if existing_id == id {
            debug!("ID's match: {:?} // {:?}", existing_id, id);
            let mut updated_task: TaskPayload = new_task.clone().into(); // convert_live_to_task(new_task.clone(), task, new_ticket);

            updated_task.service_ticket = if let Some(service) = new_ticket {
                Some(service)
            } else { task.service_ticket.clone() };
            updated_task.task_note = task.task_note.clone();

            // Calculate the diff and apply it to the existing task
            let diffs = task.diff(&updated_task);
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

pub async fn listen_data<T: DeserializeOwned + Serialize + 'static + Debug + std::marker::Unpin>(tx: Sender<(Action, T)>, resource: &str) 
    -> anyhow::Result<(), anyhow::Error> 
{
    let data_stream: Stream<Vec<T>> = DATABASE.select(resource).live().await?;
    handle_streams(data_stream, tx).await?;
    Ok(())
}

async fn handle_streams<T: Serialize + Deserialize<'static> + Debug >(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    tx: Sender<(Action, T)>
) 
    -> anyhow::Result<(), Error> 
{
    while let Some(notification) = notification_stream.next().await {
        let notif: Notification<T> = notification?;
        let data = notif.data;
        let action = notif.action;
        info!("Data: {:?}", action);
        match tx.try_send((action, data)){
            Ok(_) => info!("Sent notification"),
            Err(e) => error!("Error Sending notification {e:?}"),
        }
    }; 
    Ok(())
}

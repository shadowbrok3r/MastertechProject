use super::{DATABASE, schema::{utilities::LiveUpdate, LiveTaskPayload, TaskNotePayload, TicketPayload, TaskPayload, CONNECTED_CLIENT_TABLE, TASK_NOTE_TABLE, TASK_TABLE}};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use surrealdb::{method::Stream, Action, Notification};
use std::{collections::HashMap, fmt::Debug};
use crossbeam::channel::Sender;
use futures::StreamExt;
use log::{debug, info};

use anyhow::Error;

pub fn handle_live_data((action, data): (Action, LiveTaskPayload), existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) 
    -> anyhow::Result<(), anyhow::Error>
{
    match action{
        Action::Create => {
            data.handle_live_create(existing_tasks, new_ticket)?;
        },
        Action::Update => {
            data.handle_live_delete(existing_tasks, new_ticket)?;
        },
        Action::Delete => {
            data.handle_live_update(existing_tasks, new_ticket)?;
        },
        _ => {},
    }
    Ok(())
}

pub fn handle_live_notes(
    (action, data): (Action, TaskNotePayload), 
    existing_task: &mut TaskPayload
) 
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
            if let Some(notes) = &mut existing_task.task_note{

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


macro_rules! update_fields {
    ($vec:expr, $updated:expr, $($field:ident),+) => {
        if let Some(existing) = $vec.iter_mut().find(|foo| foo.id == $updated.id) {
            $(
                if existing.$field != $updated.$field {
                    existing.$field = $updated.$field.clone();
                }
            )+
        }
    };
}

// extern crate proc_macro;
// use proc_macro::TokenStream;
// use quote::quote;
// use syn::{parse_macro_input, DeriveInput};

// #[proc_macro_derive(UpdateFields)]
// pub fn update_fields_derive(input: TokenStream) -> TokenStream {
//     let input = parse_macro_input!(input as DeriveInput);
//     let name = input.ident;
    
//     let fields = if let syn::Data::Struct(data) = input.data {
//         data.fields.iter().map(|f| {
//             let ident = &f.ident;
//             quote! {
//                 if self.#ident != other.#ident {
//                     self.#ident = other.#ident.clone();
//                 }
//             }
//         })
//     } else {
//         unimplemented!();
//     };
    
//     let expanded = quote! {
//         impl #name {
//             pub fn update_fields(&mut self, other: &Self) {
//                 #(#fields)*
//             }
//         }
//     };
    
//     TokenStream::from(expanded)
// }


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

pub fn handle_live_delete<T: Serialize + for<'a> Deserialize<'a> + Debug + PartialEq>(existing_data: &mut Vec<T>, new_data: T) -> anyhow::Result<(), anyhow::Error> {
    debug!("Data was Deleted: {:?}", new_data);
    let index = existing_data.iter().position(|x| *x == new_data);
    if let Some(idx) = index {
        info!("Deleting @ {idx}");
        existing_data.remove(idx);
    }
    Ok(())
}

// impl LiveUpdate for TaskNotePayload {
//     fn handle_live_create(self, existing_tasks: &mut Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error>{
//         debug!("Data was Created: {:?}", self);
//         update_or_insert(existing_tasks, self)?;
//         Ok(())
//     }
//     fn handle_live_update(self, _existing_tasks: &mut Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error>{
//         debug!("Data was Updated: {:?}", self);
//         Ok(())
//     }
//     fn handle_live_delete(self, existing_tasks: &mut Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error>{
//         debug!("Data was Deleted: {:?}", self);
//         update_or_insert(existing_tasks, self)?;
//         Ok(())
//     }
// }


impl LiveUpdate for LiveTaskPayload {
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Created: {:?}", self);
        update_or_insert(existing_tasks, self, new_ticket)?;
        Ok(())
    }
    
    fn handle_live_update(self, _existing_tasks: &mut Vec<TaskPayload>, _new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        debug!("Data was Updated: {:?}", self);
        Ok(())
    }
    
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        // debug!("Data was Deleted: {:?}", self);
        update_or_insert(existing_tasks, self, new_ticket)?;
        Ok(())
    }
}

pub fn update_or_insert_notes(
    new_note: TaskNotePayload,
    task: &mut TaskPayload
) -> anyhow::Result<(), anyhow::Error> {
    if let Some(ref task_id) = new_note.task_id {
        if let Some(existing_task_id) = &task.id {
            if existing_task_id == task_id {
                // Check if the note ID already exists
                // let notes = task.task_note.get_or_insert_with(Vec::new);
                if let Some(notes) = task.task_note.as_mut(){
                    let x = new_note.note.is_empty() ;
                    let y = new_note.created_at.is_empty();
                    let z = new_note.everest_initials.is_empty();
                    
                    if notes.iter().any(|note| 
                        note.id.as_ref().unwrap().0.id != new_note.id.as_ref().unwrap().0.id && !x && !y && !z
                    ) {
                        notes.push(new_note.clone());
                        debug!("Contains notes already, inserting new: {new_note:?}");
                    }
                } else {
                    let mut vec = Vec::new();
                    vec.push(new_note.clone());
                    debug!("there were no existing notes. creating note: {:?}", vec);
                    task.task_note = Some(vec);
                }

            }
        }
        // if updated { debug!("Note updated or inserted successfully."); } 
        // else { debug!("Task ID not found or note already exists."); }
    }
    Ok(())
}

pub fn update_or_insert(
    tasks: &mut Vec<TaskPayload>, 
    new_task: LiveTaskPayload,
    new_ticket: Option<TicketPayload>
) -> anyhow::Result<(), anyhow::Error>{
    if let Some(ref id) = new_task.id {
        let mut updated = false;

        for task in tasks.iter_mut() {
            if let Some(existing_id) = &task.id {
                if existing_id == id{
                    info!("ID's match: {:?} // {:?}", existing_id, id);
                    let updated_task = convert_live_to_task(new_task.clone(), task, new_ticket);
                    *task = updated_task;
                    updated = true;
                    break;
                }
            }
        }

        if !updated {
            info!("data was NOT updated"); // TODO Do we want to 'update' the task in this case?
            let new_task_converted = convert_live_to_task(new_task, &TaskPayload::default(), None);  
            // if let Some(ticket) = new_ticket{
            //     new_task_converted.service_ticket = Some(ticket.clone());
            //     new_task_converted.service_number = Some(ticket.service_number);
            // }
            info!("new_task_converted: {new_task_converted:?}");
            // Insert the new task if it does not exist
            tasks.push(new_task_converted);
        }
    } else {
        debug!("there was NO task id");
        let new_task_converted = convert_live_to_task(new_task, &TaskPayload::default(), None);
        debug!("new_task_converted: {new_task_converted:?}");
        // If the new task does not have an ID, insert it
        tasks.push(new_task_converted);
    }
    Ok(())
}

pub fn update_or_insert_layout(
    tasks: &mut Vec<TaskPayload>, 
    new_task: LiveTaskPayload,
    new_ticket: Option<TicketPayload>,
    task_to_replace: &mut TaskPayload
) -> anyhow::Result<(), anyhow::Error>{
    if let Some(ref id) = new_task.id {
        let mut updated = false;

        for task in tasks.iter_mut() {
            if let Some(existing_id) = &task.id {
                if existing_id == id{
                    debug!("ID's match: {:?} // {:?}", existing_id, id);
                    let updated_task = convert_live_to_task(new_task.clone(), task, new_ticket);
                    *task = updated_task.clone();
                    *task_to_replace = updated_task;
                    updated = true;
                    break;
                }
            }
        }

        if !updated {
            debug!("data was NOT updated"); // TODO Do we want to 'update' the task in this case?
            let new_task_converted = convert_live_to_task(new_task, &TaskPayload::default(), None);  
            // if let Some(ticket) = new_ticket{
            //     new_task_converted.service_ticket = Some(ticket.clone());
            //     new_task_converted.service_number = Some(ticket.service_number);
            // }
            debug!("new_task_converted: {new_task_converted:?}");
            // Insert the new task if it does not exist
            tasks.push(new_task_converted);
        }
    } else {
        debug!("there was NO task id");
        let new_task_converted = convert_live_to_task(new_task, &TaskPayload::default(), None);
        debug!("new_task_converted: {new_task_converted:?}");
        // If the new task does not have an ID, insert it
        tasks.push(new_task_converted);
    }
    Ok(())
}


pub fn convert_live_to_task(live_task: LiveTaskPayload, existing_task: &TaskPayload, ticket: Option<TicketPayload>) -> TaskPayload {

    let service_ticket = if let Some(service) = ticket {
        Some(service)
    } else { existing_task.service_ticket.clone() };

    // let notes = if let Some(existing_notes) = live_task.task_note{
    //     info!("live_task.task_note: {:?}", live_task.task_note.clone());
    // // } else { 
    //     info!("existing_task.task_note.clone() : {:?}", existing_task.task_note.clone());
    // };
    TaskPayload {
        id: live_task.id,
        task_name: live_task.task_name,
        service_ticket, // Preserve the existing service_ticket
        everest_initials: live_task.everest_initials,
        task_description: live_task.task_description,
        assignee: live_task.assignee,
        service_number: live_task.service_number,
        due_date: live_task.due_date,
        priority: live_task.priority,
        task_note: existing_task.task_note.clone(), // Preserve the existing task_note
        completed: live_task.completed,
        status: live_task.status
    }
}

pub async fn listen_data<T>(tx: Sender<(Action, T)>) -> anyhow::Result<(), anyhow::Error> 
    where T: DeserializeOwned + Serialize + 'static + Debug + std::marker::Unpin 
{
    let client_stream: Stream<Vec<T>> = DATABASE.select(CONNECTED_CLIENT_TABLE).live().await?;
    handle_streams(client_stream, tx).await?;
    Ok(())
}

pub async fn listen_task_notes(tx: Sender<(Action, TaskNotePayload)>) -> anyhow::Result<(), anyhow::Error> {
    info!("Listening to task notes");
    let note_stream: Stream<Vec<TaskNotePayload>> = DATABASE.select(TASK_NOTE_TABLE).live().await?;
    handle_streams(note_stream, tx).await?;
    Ok(())
}

pub async fn listen_tasks(tx: Sender<(Action, LiveTaskPayload)>) -> anyhow::Result<(), anyhow::Error> {
    let task_stream: Stream<Vec<LiveTaskPayload>> = DATABASE.select(TASK_TABLE).live().await?;
    handle_streams(task_stream, tx).await?;
    Ok(())
}

async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    tx: Sender<(Action, T)>
) -> anyhow::Result<(), Error> 
    where T: Serialize + Deserialize<'static> + Debug 
{
    while let Some(notification) = notification_stream.next().await {
        let notif: Notification<T> = notification?;
        let data = notif.data;
        let action = notif.action;
        info!("Data: {:?}", action);
        match tx.try_send((action, data)){
            Ok(_) => info!("Sent notification"),
            Err(e) => info!("Error Sending notification {e:?}"),
        }
    }; 
    Ok(())
}
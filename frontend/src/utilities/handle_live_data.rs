use database::{schema::TaskPayload, DATABASE};
use futures::StreamExt;
use database::schema::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use surrealdb::{method::Stream, Action, Notification};
use wasm_bindgen_futures::spawn_local;
use log::{info, error};
use crossbeam::channel::Sender;
use surrealdb::engine::remote::ws::Client;
use std::{collections::HashMap, fmt::Debug};
use super::LiveUpdate;


pub fn handle_live_data((action, data): (Action, LiveTaskPayload), existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
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
            info!("Data: {data:?}");
            update_or_insert_notes(data, existing_task)?;
        },
        _ => {},
    }
    Ok(())
}


pub fn handle_live_create<T: Serialize + for<'a> Deserialize<'a> + Debug>(_existing_data: &mut HashMap<String, T>, new_data: T) -> anyhow::Result<(), anyhow::Error> {
    info!("Data was Created: {:?}", new_data);

    Ok(())
}

pub fn handle_live_update<T: Serialize + for<'a> Deserialize<'a> + Debug>(_existing_data: &mut HashMap<String, T>, new_data: T) -> anyhow::Result<(), anyhow::Error> {
    info!("Data was Updated: {:?}", new_data);

    Ok(())
}

pub fn handle_live_delete<T: Serialize + for<'a> Deserialize<'a> + Debug>(_existing_data: &mut HashMap<String, T>, new_data: T) -> anyhow::Result<(), anyhow::Error> {
    info!("Data was Deleted: {:?}", new_data);

    Ok(())
}

// impl LiveUpdate for TaskNotePayload {
//     fn handle_live_create(self, existing_tasks: &mut Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error>{
//         info!("Data was Created: {:?}", self);
//         update_or_insert(existing_tasks, self)?;
//         Ok(())
//     }
//     fn handle_live_update(self, _existing_tasks: &mut Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error>{
//         info!("Data was Updated: {:?}", self);
//         Ok(())
//     }
//     fn handle_live_delete(self, existing_tasks: &mut Vec<TaskNotePayload>) -> anyhow::Result<(), anyhow::Error>{
//         info!("Data was Deleted: {:?}", self);
//         update_or_insert(existing_tasks, self)?;
//         Ok(())
//     }
// }


impl LiveUpdate for LiveTaskPayload {
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Created: {:?}", self);
        update_or_insert(existing_tasks, self, new_ticket)?;
        Ok(())
    }
    
    fn handle_live_update(self, _existing_tasks: &mut Vec<TaskPayload>, _new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        info!("Data was Updated: {:?}", self);
        Ok(())
    }
    
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>{
        // info!("Data was Deleted: {:?}", self);
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
                        info!("Contains notes already, inserting new: {new_note:?}");
                    }
                } else {
                    let mut vec = Vec::new();
                    vec.push(new_note.clone());
                    info!("there were no existing notes. creating note: {:?}", vec);
                    task.task_note = Some(vec);
                }

            }
        }
        // if updated { info!("Note updated or inserted successfully."); } 
        // else { info!("Task ID not found or note already exists."); }
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
                    let updated_task = convert_live_to_task(new_task.clone(), task);
                    *task = updated_task;
                    updated = true;
                    break;
                }
            }
        }

        if !updated {
            info!("data was NOT updated"); // TODO Do we want to 'update' the task in this case?
            // todo!();
            let mut new_task_converted = convert_live_to_task(new_task, &TaskPayload::default());  
            if let Some(ticket) = new_ticket{
                new_task_converted.service_ticket = Some(ticket.clone());
                new_task_converted.service_number = Some(ticket.service_number);
            }
            info!("new_task_converted: {new_task_converted:?}");
            // Insert the new task if it does not exist
            tasks.push(new_task_converted);
        }
    } else {
        info!("there was NO task id");
        let new_task_converted = convert_live_to_task(new_task, &TaskPayload::default());
        info!("new_task_converted: {new_task_converted:?}");
        // If the new task does not have an ID, insert it
        tasks.push(new_task_converted);
    }
    Ok(())
}

pub fn convert_live_to_task(live_task: LiveTaskPayload,existing_task: &TaskPayload) -> TaskPayload {
    TaskPayload {
        id: live_task.id,
        task_name: live_task.task_name,
        service_ticket: existing_task.service_ticket.clone(), // Preserve the existing service_ticket
        everest_initials: live_task.everest_initials,
        task_description: live_task.task_description,
        assignee: live_task.assignee,
        service_number: live_task.service_number,
        due_date: live_task.due_date,
        priority: live_task.priority,
        task_note: existing_task.task_note.clone(), // Preserve the existing task_note
        completed: live_task.completed,
        status: live_task.status,
        dep: live_task.dep,
    }
}

pub fn listen_data<T>(tx: Sender<(Action, T)>) 
    where T: DeserializeOwned + Serialize + 'static + Debug + std::marker::Unpin
{
    spawn_local(async move {
        let client_stream: Stream<Client, Vec<T>> = DATABASE.select(CONNECTED_CLIENT_TABLE).live().await.unwrap();
        handle_streams(client_stream, tx).await;
    });
}

pub fn listen_task_notes(tx: Sender<(Action, TaskNotePayload)>) 
    // where T: DeserializeOwned + Serialize + 'static + Debug + marker::Unpin
{
    spawn_local(async move {
        let task_stream: Stream<Client, Vec<TaskNotePayload>> = DATABASE.select(TASK_NOTE_TABLE).live().await.unwrap();
        handle_streams(task_stream, tx).await;
    });
}

pub fn listen_tasks(tx: Sender<(Action, LiveTaskPayload)>) 
    // where T: DeserializeOwned + Serialize + 'static + Debug + marker::Unpin
{
    spawn_local(async move {
        let task_stream: Stream<Client, Vec<LiveTaskPayload>> = DATABASE.select(TASK_TABLE).live().await.unwrap();
        handle_streams(task_stream, tx).await;
    });
}

async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    tx: Sender<(Action, T)>
) where T: Serialize + Deserialize<'static> + Debug
{
    while let Some(notification) = notification_stream.next().await {
        match notification{
            Ok(notification) => {
                let data = notification.data;
                let action = notification.action;
                info!("Data: {data:?}");
                match tx.try_send((action, data)){
                    Ok(_) => info!("Sent notification"),
                    Err(e) => error!("Error sending task data: {e:?}")
                }
            },
            Err(err) => error!("Error: {err:?}")
        };
    }; 
}


// pub trait IntoTaskPayload {
//     fn into_task_payload(self) -> TaskPayload;
// }

// impl IntoTaskPayload for LiveTaskPayload {
//     fn into_task_payload(self) -> TaskPayload {
//         // Parse the service_ticket field
//         let service_ticket = self.service_ticket.map(|ticket_str| {
//             // Implement parsing logic for TicketPayload from ticket_str
//             // For example:
//             serde_json::from_str::<TicketPayload>(&ticket_str.0.to_string()).unwrap()
//         });

//         // Parse the task_note field
//         let task_note = self.task_note.map(|notes| {
//             notes.into_iter().map(|note_str| {
//                 // Implement parsing logic for TaskNotePayload from note_str
//                 // For example:
//                 serde_json::from_str::<TaskNotePayload>(&note_str.0.to_string()).unwrap()
//             }).collect()
//         });

//         TaskPayload {
//             id: self.id,
//             task_name: self.task_name,
//             service_ticket,
//             everest_initials: self.everest_initials,
//             task_description: self.task_description,
//             assignee: self.assignee,
//             service_number: self.service_number,
//             due_date: self.due_date,
//             priority: self.priority,
//             task_note,
//             completed: self.completed,
//             status: self.status,
//             dep: self.dep,
//         }
//     }
// }

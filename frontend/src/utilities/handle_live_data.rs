use database::schema::TaskPayload;
use futures::StreamExt;
use database::{schema::*, Database};
use surrealdb::{method::Stream, Action, Notification};
use wasm_bindgen_futures::spawn_local;
use log::{info, error};
use crossbeam::channel::Sender;
use surrealdb::engine::remote::ws::Client;
use super::LiveUpdate;


pub fn handle_live_data(data: (Action, LiveTaskPayload), existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>{
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

impl LiveUpdate for LiveTaskPayload {
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
        update_or_insert(existing_tasks, self)?;
        Ok(())
    }
}


pub fn update_or_insert(
    tasks: &mut Vec<TaskPayload>, 
    new_task: LiveTaskPayload,
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
            info!("data was NOT updated");
            let new_task_converted = convert_live_to_task(new_task, &TaskPayload::default());    
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

pub fn convert_live_to_task(
    live_task: LiveTaskPayload,
    existing_task: &TaskPayload,
) -> TaskPayload {
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



pub fn listen_tasks(db: Database, tx: Sender<(Action, LiveTaskPayload)>) 
    // where T: DeserializeOwned + Serialize + 'static + Debug + marker::Unpin
{
    spawn_local(async move {
        let task_stream: Stream<Client, Vec<LiveTaskPayload>> = db.database.select(TASK_TABLE).live().await.unwrap();
        handle_streams(task_stream, tx).await;
    });
}



async fn handle_streams(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<LiveTaskPayload>, surrealdb::Error>> + Unpin,
    tx: Sender<(Action, LiveTaskPayload)>
){ //  where T: Serialize + Deserialize<'static> + Debug
    while let Some(notification) = notification_stream.next().await {
        match notification{
            Ok(notification) => {
                let data = notification.data;
                let action = notification.action;
                match tx.send((action, data)){
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


use database::{schema::{ClientId, ComputerData, ConnectedClient, CustomerData, LiveTaskPayload, Record, Store, TaskId, TaskNotePayload, TaskPayload, TicketPayload, User, TASK_NOTE_TABLE, TASK_TABLE}, DATABASE};
use crossbeam::channel::Sender;
use log::{debug, error, info};
use mtechserver::webworker::LiveOutput;
use surrealdb::{sql::{Id, Thing}, Action};
use serde::{Deserialize, Serialize};
use surrealdb::opt::RecordId;
use wasm_bindgen_futures::spawn_local;
use std::fmt::Debug;
use crate::app_state::NewTicketChannel;

use super::Task;

pub async fn get_tasks(tx: Sender<Vec<TaskPayload>>) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_tasks");
    let query = format!("SELECT * FROM task FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note");
    let query_results: Vec<TaskPayload> = DATABASE.query(query).await?.take(0)?;
    tx.try_send(query_results)?;
    Ok(())
}

pub async fn get_associated_ticket(tx: Sender<NewTicketChannel>, new_task: (Action, LiveTaskPayload)) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_associated_ticket");
    let service_num = new_task.1.clone().service_number.unwrap_or_default();
    DATABASE.set("service_num", service_num).await?;
    let ticket: Option<TicketPayload> = DATABASE.query(format!("SELECT * FROM service_order WHERE service_number == $service_num FETCH computer, customer")).await?.take(0)?;
    debug!("ticket: {:?}", ticket);
    let new_ticket = ticket.unwrap_or_default();
    let chnnl = NewTicketChannel { new_ticket, new_task };
    tx.try_send(chnnl)?;
    Ok(())
}

pub async fn get_customer_data() -> anyhow::Result<LiveOutput, anyhow::Error> { // tx: Sender<CustomerData>
    debug!("get_customers");
    let customers: Vec<CustomerData> = DATABASE.query("SELECT * FROM customer").await?.take(0)?;
    DATABASE.set("id", "value");
    let computers: Vec<ComputerData> = DATABASE.query("SELECT * FROM computer where customer == $id").await?.take(0)?;
    let tickets: Vec<TicketPayload> = DATABASE.query("SELECT * FROM service_order where customer == $id").await?.take(0)?;
    let tasks: Vec<TaskPayload> = DATABASE.query("SELECT * FROM task where service_order == $id").await?.take(0)?;

    let output = LiveOutput{ customers, computers, tickets, tasks };
    Ok(output)
}



pub async fn get_associated_task_notes(tx: Sender<TaskNotePayload>, note_id: Id) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_associated_task_notes");
    DATABASE.set("id", note_id).await?;
    let note: Option<TaskNotePayload> = DATABASE.query(format!("SELECT * FROM task_note WHERE id == $id")).await?.take(0)?;
    debug!("note: {:?}", note);
    let new_note = note.unwrap_or_default();
    tx.try_send(new_note)?;
    Ok(())
}

pub async fn get_store_users(tx: Sender<Vec<User>>, store: Store) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_store_users");
    DATABASE.set("store", store).await?;
    let data: Vec<User> = DATABASE.query("SELECT name, store, everest_initials, id, email FROM user WHERE store == $store").await?.take(0)?;
    tx.try_send(data)?;
    Ok(())
}

pub async fn get_connected_clients(tx: Sender<Vec<ConnectedClient>>, user_id: User) -> anyhow::Result<(), anyhow::Error> {
    debug!("get_connected_clients");
    DATABASE.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = DATABASE.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;
    tx.try_send(query)?;
    Ok(())
}

pub async fn disconnect_client(tx: Sender<Vec<ClientId>>, id: ClientId) -> anyhow::Result<(), anyhow::Error> {
    DATABASE.set("id", id.0.id).await?;
    let query: Vec<ClientId> = DATABASE.update("UPDATE connected_client SET connected = false WHERE id == $id").await?;
    tx.try_send(query)?;

    Ok(())
}

pub async fn modify_connected_client(tx: Sender<Vec<ConnectedClient>>, user_id: User) -> anyhow::Result<(), anyhow::Error> {
    DATABASE.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = DATABASE.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;
    tx.try_send(query)?;
    Ok(())
}

pub async fn delete_task(id: Thing) -> anyhow::Result<(), anyhow::Error> {
    let id = id.clone();
    info!("deleting id: {id:?}");
    DATABASE.set("id", id.id.clone()).await?;
    let _y: Option<TaskPayload> = DATABASE.delete((TASK_TABLE, id.id)).await?;
    Ok(())
}


pub trait TaskNoteMod {
    async fn delete_note(&mut self) -> anyhow::Result<(), anyhow::Error>;
}

impl TaskNoteMod for TaskNotePayload {
    async fn delete_note(&mut self) -> anyhow::Result<(), anyhow::Error> {
        let id = self.id.clone();
        if let Some(id) = id {
            info!("deleting id: {:?}", id.clone());
            DATABASE.set("id", id.0.id.clone()).await?;
            let y: Option<Record> = DATABASE.delete((TASK_NOTE_TABLE, id.0.id)).await?;
            info!("Deleted note: {:?}", y);
        }
        Ok(())
    }
}

pub async fn update_task_notes(new_msg: String, task_id: TaskId) -> anyhow::Result<(), anyhow::Error>{
    let id = task_id.clone();
    let task_note = TaskNotePayload { task_id: Some(id), note: new_msg, ..Default::default() };

    let query = format!("CREATE task_note CONTENT $note");
    DATABASE.set("note", task_note).await.unwrap();
    let update_task: Vec<Record> = DATABASE
        .query(query)
        .await?
        .take(0)?;

    info!("Updated notes: {update_task:?}");
    Ok(())
}

impl Task for TaskPayload{
    fn get_computer_data<T>(&mut self, tx: Sender<Option<T>>) //-> anyhow::Result<(), anyhow::Error> 
        where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
    {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            debug!("get_data: {get_data:#?}");

                match tx.try_send(get_data){
                    Ok(_) => debug!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
        });
    }

    fn get_customer_data<T>(&mut self, tx: Sender<Option<T>>) //-> anyhow::Result<(), anyhow::Error> 
        where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
    {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            debug!("get_data: {get_data:#?}");

                match tx.try_send(get_data){
                    Ok(_) => debug!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
        });
        
    }
    
    fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>) //-> anyhow::Result<(), anyhow::Error> 
        where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
    {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT * FROM task_note WHERE id={id}"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            debug!("get_data: {get_data:#?}");

                match tx.try_send(get_data){
                    Ok(_) => debug!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
        });
    }

    fn get_ticket_payload<T>(&mut self, tx: Sender<Option<T>>)//-> anyhow::Result<(), anyhow::Error> 
        where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
    {
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            
            let get_data: Option<T> = DATABASE
                .query(format!("SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id={id}"))
                .await
                .unwrap()
                .take(0).unwrap();

            match tx.try_send(get_data){
                Ok(_) => debug!("Sent data"),
                Err(e) => error!("Error sending data: {e:?}")
            };
        });
    }
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>)//-> anyhow::Result<(), anyhow::Error> 
    //     where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
    // {
    //     let id: RecordId = self.service_ticket.clone().unwrap().clone().0;
    //     spawn_local(async move {
    //         let query = format!(
    //             "SELECT * FROM service_order WHERE id={id}"
    //         );
    //         let get_data: Option<T> = db
    //             .database
    //             .query(query)
    //             .await
    //             .unwrap()
    //             .take(0).unwrap();
    //         debug!("get_data: {get_data:#?}");
    //             match tx.try_send(get_data){
    //                 Ok(_) => debug!("Sent data"),
    //                 Err(e) => error!("Error sending data: {e:?}")
    //             };
    //     });
    // }
    
}
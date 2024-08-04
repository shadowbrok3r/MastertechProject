
use async_trait::async_trait;
use database::{schema::{ClientId, ComputerData, ConnectedClient, CustomerData, LiveTaskPayload, Notification, Record, Store, TaskId, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User, TASK_NOTE_TABLE, TASK_TABLE}, DATABASE};
use crossbeam::channel::Sender;
use log::{debug,info};
use mtechserver::live_worker::LiveOutput;
use surrealdb::{sql::{Id, Thing}, Action};
use serde::{Deserialize, Serialize};
use surrealdb::opt::RecordId;
// use wasm_bindgen_futures::spawn_local;
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

pub async fn get_customer_data(tx: Sender<LiveOutput>) -> anyhow::Result<(), anyhow::Error> { // tx: Sender<CustomerData>
    debug!("get_customers");
    let customers: Vec<CustomerData> = DATABASE.query("SELECT * FROM customer").await?.take(0)?;
    DATABASE.set("id", "value").await?;
    let computers: Vec<ComputerData> = DATABASE.query("SELECT * FROM computer").await?.take(0)?;
    let tickets: Vec<TicketData> = DATABASE.query("SELECT * FROM service_order").await?.take(0)?;
    let output = LiveOutput{ customers, computers, tickets };
    tx.try_send(output)?;
    Ok(())
}

pub async fn get_notifications(tx: Sender<Vec<Notification>>, id: Thing) -> anyhow::Result<(), anyhow::Error> { // tx: Sender<CustomerData>
    debug!("get_notifications");
    DATABASE.set("id", id).await?;
    let notifications: Vec<Notification> = DATABASE.query("SELECT * FROM notification WHERE user == $id").await?.take(0)?;
    // info!("Notifications: {:?}", notifications.clone());
    tx.try_send(notifications)?;
    Ok(())
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
    DATABASE.set("store", store).await?; // $auth.store
    let data: Vec<User> = DATABASE.query("SELECT name, store, everest_initials, id, email, minio_access_key, minio_secret_key FROM user WHERE store == $store").await?.take(0)?;
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


#[async_trait]
pub trait TaskNoteMod {
    async fn delete_note(&mut self) -> anyhow::Result<(), anyhow::Error>;
}

#[async_trait]
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
 
#[async_trait]
impl Task for TaskPayload{
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error> 
    {
        let id: RecordId = self.id.clone().unwrap().0;
            let query = format!(
                "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error> 
    {
        let id: RecordId = self.id.clone().unwrap().0;
            let query = format!(
                "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            debug!("get_data: {get_data:#?}");
        Ok(get_data)
        
    }
    
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error> 
    {
        let id: RecordId = self.id.clone().unwrap().0;
            let query = format!(
                "SELECT * FROM task_note WHERE id={id}"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error> 
    {
        let id: RecordId = self.id.clone().unwrap().0;
            
            let get_data: Option<T> = DATABASE
                .query(format!("SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id={id}"))
                .await
                .unwrap()
                .take(0).unwrap();
        Ok(get_data)
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
use crate::{DATABASE, schema::{Priority, Status, Cmd, SystemInformation, ClientId, ConnectedClient, Record, Store, TaskId, TaskNotePayload, TaskPayload, User, TASK_NOTE_TABLE, TASK_TABLE}};
use async_trait::async_trait;
use surrealdb::sql::{Id, Thing};
use crossbeam::channel::Sender;
use anyhow::{Result, Error};
use log::{info, debug};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use super::{Notification, TicketPayload};

pub trait FilterTasks{ 
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: &String) -> Vec<TaskPayload>;
    fn filter_by_my_store(&self, assignees: &Vec<User>, current_user: &User) -> Vec<TaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(&self, name: T, search_input: String) -> Vec<TaskPayload>;
}

pub trait Sortable{
    fn sort_task_payloads(&mut self) -> &mut Vec<TaskPayload>;
}

pub trait LiveUpdate{
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>, new_ticket: Option<TicketPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
}

#[async_trait]
pub trait Task{ // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error>;
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>);
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self) -> anyhow::Result<Option<T>, anyhow::Error>;
    // fn create_data(&mut self, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn get_data(&mut self, data: T)    -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn modify_data(&mut self, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn delete_data(&mut self, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
}

pub async fn query_user_from_email(email: String) -> Result<User, Error>{
    let query = format!("SELECT id, name, everest_initials, email, store FROM user WHERE email == $email"); //  OR email == $email

    if email.contains("@pclaptops.com"){
        DATABASE.set("email", email.clone()).await?;
    } else {
        DATABASE.set("email", format!("{}@pclaptops.com", email.clone())).await?;
    }

    info!("Email: {}", email);
    let user: Option<User> = DATABASE.query(query).await?.take(0)?;
    info!("user: {:?}", user.clone());
    // let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
    Ok(user.unwrap())
}

pub async fn query_id<'a, T>(table: &'a str, id: T) 
    -> Result<Option<Record>, Error>
        where T: Serialize + Debug + Clone
{
    let query = format!("SELECT * FROM $table WHERE id == $id");
    DATABASE.set("id", id).await?;
    DATABASE.set("table", table).await?;
    let record: Option<Record> = DATABASE.query(query.clone()).await?.take(0)?;
    info!("Query: {:?}  // {}", record, query);
    Ok(record)
}

pub async fn check_id_existence<'a, T>(table: &'a str, id: T) 
    -> Result<Option<bool>, Error>
        where T: Serialize + Debug + Clone
{
    let query = format!(r#"
        LET $query = (SELECT $id FROM $table);
        IF $query != NULL || NONE {{ true }} ELSE {{ false }};
    "#);
    DATABASE.set("id", id).await?;
    DATABASE.set("table", table).await?;
    let record: Option<bool> = DATABASE.query(query.clone()).await?.take(1)?;
    info!("Query: {:?}  // {}", record, query);
    Ok(record)
}

pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    bincode::serialize(system_info).expect("Failed to serialize SystemInformation")
}

pub fn _deserialize_system_info(bytes: &[u8]) -> SystemInformation {
    bincode::deserialize(bytes).expect("Failed to deserialize SystemInformation")
}

pub fn deserialize_command(bytes: &[u8]) -> Cmd {
    bincode::deserialize(bytes).expect("Failed to deserialize Cmd")
}

pub async fn get_tasks(tx: Sender<Vec<TaskPayload>>) -> Result<(), Error> {
    debug!("get_tasks");
    let query = format!("SELECT * FROM task FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note");
    let query_results: Vec<TaskPayload> = DATABASE.query(query).await?.take(0)?;
    tx.try_send(query_results)?;
    Ok(())
}


pub async fn get_associated_task_notes(tx: Sender<TaskNotePayload>, note_id: Id) -> Result<(), Error> {
    debug!("get_associated_task_notes");
    DATABASE.set("id", note_id).await?;
    let note: Option<TaskNotePayload> = DATABASE.query(format!("SELECT * FROM task_note WHERE id == $id")).await?.take(0)?;
    debug!("note: {:?}", note);
    let new_note = note.unwrap_or_default();
    tx.try_send(new_note)?;
    Ok(())
}

pub async fn get_store_users(tx: Sender<Vec<User>>, store: Store) -> Result<(), Error> {
    debug!("get_store_users");
    DATABASE.set("store", store).await?;
    let data: Vec<User> = DATABASE.query("SELECT name, store, everest_initials, id, email FROM user WHERE store == $store").await?.take(0)?;
    tx.try_send(data)?;
    Ok(())
}

pub async fn get_connected_clients(tx: Sender<Vec<ConnectedClient>>, user_id: User) -> Result<(), Error> {
    debug!("get_connected_clients");
    DATABASE.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = DATABASE.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;
    tx.try_send(query)?;
    Ok(())
}

pub async fn disconnect_client(tx: Sender<Vec<ClientId>>, id: ClientId) -> Result<(), Error> {
    DATABASE.set("id", id.0.id).await?;
    let query: Vec<ClientId> = DATABASE.update("UPDATE connected_client SET connected = false WHERE id == $id").await?;
    tx.try_send(query)?;

    Ok(())
}

pub async fn modify_connected_client(tx: Sender<Vec<ConnectedClient>>, user_id: User) -> Result<(), Error> {
    DATABASE.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = DATABASE.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;
    tx.try_send(query)?;
    Ok(())
}

pub async fn delete_task(id: Thing) -> Result<(), Error> {
    let id = id.clone();
    info!("deleting id: {id:?}");
    DATABASE.set("id", id.id.clone()).await?;
    let _y: Option<TaskPayload> = DATABASE.delete((TASK_TABLE, id.id)).await?;
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

// pub async fn get_associated_ticket(tx: Sender<NewTicketChannel>, new_task: (Action, LiveTaskPayload)) -> Result<(), Error> {
//     debug!("get_associated_ticket");
//     let service_num = new_task.1.clone().service_number.unwrap_or_default();
//     DATABASE.set("service_num", service_num).await?;
//     let ticket: Option<TicketPayload> = DATABASE.query(format!("SELECT * FROM service_order WHERE service_number == $service_num FETCH computer, customer")).await?.take(0)?;
//     debug!("ticket: {:?}", ticket);
//     let new_ticket = ticket.unwrap_or_default();
//     let chnnl = NewTicketChannel { new_ticket, new_task };
//     tx.try_send(chnnl)?;
//     Ok(())
// }

#[async_trait]
pub trait TaskNoteMod {
    async fn delete_note(&mut self) -> Result<(), Error>;
}

#[async_trait]
impl TaskNoteMod for TaskNotePayload {
    async fn delete_note(&mut self) -> Result<(), Error> {
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

pub async fn update_task_notes(new_msg: String, task_id: TaskId) -> Result<(), Error>{
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


// #[async_trait]
// pub trait Updatable { 
//     async fn update_completed(&self, completed: bool) -> Result<(), Error>;
//     async fn update_due_date(&self, due_date: String) -> Result<(), Error>;
//     async fn update_assignee_initials(&self, initials: String) -> Result<(), Error>;
//     async fn update_task_name(&self, name: String) -> Result<(), Error>;
//     async fn update_status(&self, status: Status) -> Result<(), Error>;
//     async fn update_dep(&self, store: Store) -> Result<(), Error>;
//     async fn update_priority(&self, priority: Option<Priority>) -> Result<(), Error>;
//     async fn update_task_description(&self, description: String) -> Result<(), Error>;
//     async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> Result<(), Error>;
//     async fn update_task_notes(&self, new_msg: String) -> Result<(), Error>;
// }

// #[async_trait]
// impl Updatable for TaskPayload {
//     async fn update_completed(&self, completed: bool) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let query = format!("UPDATE task SET completed=$completed, status=$status WHERE id=$id");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("completed", completed).await?;
//         if completed{ DATABASE.set("status", Status::Complete).await?; }
//         else{ DATABASE.set("status", Status::InRepair).await?; }
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_due_date(&self, due_date: String) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let query = format!("UPDATE task SET due_date=$date WHERE id=$id");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("date", due_date).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_assignee_initials(&self, initials: String) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let user_query = format!("SELECT id FROM user WHERE everest_initials=$initials");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("initials", initials).await?;    
//         let selected_user: Option<Record> = DATABASE.query(user_query).await?.take(0)?;
//         let query = format!("UPDATE task SET assignee=$assignee, everest_initials=$initials WHERE id=$id");
//         DATABASE.set("assignee", selected_user.unwrap().id).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
        
//     }

//     async fn update_task_name(&self, name: String) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let query = format!("UPDATE task SET task_name=$name WHERE id=$id");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("name", name).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_status(&self, status: Status) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let mut _query = String::new();
//         DATABASE.set("id", id).await?;
//         match status{
//             Status::Todo => {
//                 _query = format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
//                 DATABASE.set("status", Status::Todo).await?;
//             },
//             Status::InRepair => {
//                 _query = format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
//                 DATABASE.set("status", Status::InRepair).await?;
//             },
//             Status::Complete => {
//                 _query = format!("UPDATE task SET status=$status, completed=true WHERE id=$id");
//                 DATABASE.set("status", Status::Complete).await?;
//             },
//         }

//         let _update_task: Vec<Record> = DATABASE.query(_query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_dep(&self, dep: Store) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let query = format!("UPDATE task SET dep=$dep WHERE id=$id");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("dep", dep).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_priority(&self, priority: Option<Priority>) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let query = format!("UPDATE task SET priority=$priority WHERE id=$id");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("priority", priority.unwrap()).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_task_description(&self, description: String) -> Result<(), Error> {
//         let id: RecordId = self.id.clone().unwrap().0;
//         let query = format!("UPDATE task SET task_description=$description WHERE id=$id");
//         DATABASE.set("id", id).await?;
//         DATABASE.set("description", description).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }
    
//     async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> Result<(), Error> {
//         let id = self.service_ticket.as_ref();
//         let x = id.unwrap().id.clone().unwrap().0;
//         let query = format!("UPDATE service_order SET checkin_notes=$notes WHERE id=$id");
//         DATABASE.set("id", checkin_notes.unwrap()).await?;
//         DATABASE.set("notes", x).await?;
//         let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         Ok(())
//     }

//     async fn update_task_notes(&self, new_msg: String) -> Result<(), Error> {
//         let task_note = TaskNotePayload {
//             task_id: self.id.clone(),
//             note: new_msg,
//             ..Default::default()
//         };
        
//         let query = format!("CREATE task_note CONTENT $note");
//         DATABASE.set("note", task_note).await?;
//         let update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
//         info!("Updated notes: {update_task:?}");
//         Ok(())
//     }
// }

// impl Task for TaskPayload{
//     fn get_computer_data<T>(&mut self, tx: Sender<Option<T>>) //-> Result<(), Error> 
//         where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
//     {
//         let id: RecordId = self.id.clone().unwrap().0;
//         spawn(async move {
//             let query = format!(
//                 "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
//             );
//             let get_data: Option<T> = DATABASE
//                 .query(query)
//                 .await
//                 .unwrap()
//                 .take(0).unwrap();
//             debug!("get_data: {get_data:#?}");

//                 match tx.try_send(get_data){
//                     Ok(_) => debug!("Sent data"),
//                     Err(e) => error!("Error sending data: {e:?}")
//                 };
//         });
//     }

//     fn get_customer_data<T>(&mut self, tx: Sender<Option<T>>) //-> Result<(), Error> 
//         where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
//     {
//         let id: RecordId = self.id.clone().unwrap().0;
//         spawn(async move {
//             let query = format!(
//                 "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
//             );
//             let get_data: Option<T> = DATABASE
//                 .query(query)
//                 .await
//                 .unwrap()
//                 .take(0).unwrap();
//             debug!("get_data: {get_data:#?}");

//                 match tx.try_send(get_data){
//                     Ok(_) => debug!("Sent data"),
//                     Err(e) => error!("Error sending data: {e:?}")
//                 };
//         });
        
//     }
    
//     fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>) //-> Result<(), Error> 
//         where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
//     {
//         let id: RecordId = self.id.clone().unwrap().0;
//         spawn(async move {
//             let query = format!(
//                 "SELECT * FROM task_note WHERE id={id}"
//             );
//             let get_data: Option<T> = DATABASE
//                 .query(query)
//                 .await
//                 .unwrap()
//                 .take(0).unwrap();
//             debug!("get_data: {get_data:#?}");

//                 match tx.try_send(get_data){
//                     Ok(_) => debug!("Sent data"),
//                     Err(e) => error!("Error sending data: {e:?}")
//                 };
//         });
//     }

//     fn get_ticket_payload<T>(&mut self, tx: Sender<Option<T>>)//-> Result<(), Error> 
//         where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
//     {
//         let id: RecordId = self.id.clone().unwrap().0;
//         spawn(async move {
            
//             let get_data: Option<T> = DATABASE
//                 .query(format!("SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id={id}"))
//                 .await
//                 .unwrap()
//                 .take(0).unwrap();

//             match tx.try_send(get_data){
//                 Ok(_) => debug!("Sent data"),
//                 Err(e) => error!("Error sending data: {e:?}")
//             };
//         });
//     }
//     // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>)//-> Result<(), Error> 
//     //     where T: Serialize + for<'a> Deserialize<'a> + Debug + 'static 
//     // {
//     //     let id: RecordId = self.service_ticket.clone().unwrap().clone().0;
//     //     spawn(async move {
//     //         let query = format!(
//     //             "SELECT * FROM service_order WHERE id={id}"
//     //         );
//     //         let get_data: Option<T> = db
//     //             .database
//     //             .query(query)
//     //             .await
//     //             .unwrap()
//     //             .take(0).unwrap();
//     //         debug!("get_data: {get_data:#?}");
//     //             match tx.try_send(get_data){
//     //                 Ok(_) => debug!("Sent data"),
//     //                 Err(e) => error!("Error sending data: {e:?}")
//     //             };
//     //     });
//     // }
    
// }
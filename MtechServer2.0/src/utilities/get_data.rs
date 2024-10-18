use crate::app_state::NewTicketChannel;
use anyhow::{Error, Result};
use async_trait::async_trait;
use crossbeam::channel::Sender;
use database::{
    schema::{
        ComputerData, CustomerData, LiveTaskPayload, Record, TaskNotePayload, TaskPayload,
        TicketData, TicketPayload, User, TASK_NOTE_TABLE,
    },
    DATABASE,
};
use log::{debug, info};
use mtechserver::live_worker::LiveOutput;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use surrealdb::Action;
use surrealdb::RecordId;

use super::Task;

pub async fn get_associated_ticket(
    tx: Sender<NewTicketChannel>,
    new_task: (Action, LiveTaskPayload),
) -> Result<(), Error> {
    debug!("get_associated_ticket");
    let service_num = new_task.1.clone().service_number.unwrap_or_default();
    DATABASE.set("service_num", service_num).await?;
    let ticket: Option<TicketPayload> = DATABASE.query(format!("SELECT * FROM service_order WHERE service_number == $service_num FETCH computer, customer")).await?.take(0)?;
    debug!("ticket: {:?}", ticket);
    let new_ticket = ticket.unwrap_or_default();
    let chnnl = NewTicketChannel {
        new_ticket,
        new_task,
    };
    tx.try_send(chnnl)?;
    Ok(())
}

pub async fn get_customer_data(tx: Sender<LiveOutput>) -> Result<(), Error> {
    // tx: Sender<CustomerData>
    debug!("get_customers");
    let customers: Vec<CustomerData> = DATABASE.query("SELECT * FROM customer").await?.take(0)?;
    DATABASE.set("id", "value").await?;
    let computers: Vec<ComputerData> = DATABASE.query("SELECT * FROM computer").await?.take(0)?;
    let tickets: Vec<TicketData> = DATABASE
        .query("SELECT * FROM service_order")
        .await?
        .take(0)?;
    let output = LiveOutput {
        customers,
        computers,
        tickets,
    };
    tx.try_send(output)?;
    Ok(())
}

pub async fn get_user_from_email(email: String) -> Result<Option<User>, Error> {
    DATABASE.set("email", email).await?;
    let user_record: Option<User> = DATABASE
        .query("SELECT * FROM user WHERE email == $email")
        .await?
        .take(0)?;

    Ok(user_record)
}

#[async_trait]
pub trait TaskNoteMod {
    async fn delete_note(&mut self) -> Result<(), Error>;
}

#[async_trait]
impl TaskNoteMod for TaskNotePayload {
    async fn delete_note(&mut self) -> Result<(), Error> {
        let id = self.id.clone();
        info!("deleting id: {:?}", id.clone());
        DATABASE.set("id", id.key().to_string().clone()).await?;
        let y: Option<Record> = DATABASE.delete((TASK_NOTE_TABLE, id.key().to_string())).await?;
        info!("Deleted note: {:?}", y);
    
        Ok(())
    }
}

pub async fn update_task_notes(new_msg: String, task_id: RecordId) -> Result<(), Error> {
    let id = task_id.clone();
    let task_note = TaskNotePayload {
        task_id: Some(id),
        note: new_msg,
        ..Default::default()
    };

    let query = format!("CREATE task_note CONTENT $note");
    DATABASE.set("note", task_note).await.unwrap();
    let update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;

    info!("Updated notes: {update_task:?}");
    Ok(())
}

#[async_trait]
impl Task for TaskPayload {
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();
        let query = format!(
            "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
        );
        let get_data: Option<T> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();
        let query = format!(
            "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
        );
        let get_data: Option<T> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();
        let query = format!("SELECT * FROM task_note WHERE id={id}");
        let get_data: Option<T> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();

        let get_data: Option<T> = DATABASE
                .query(format!("SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id={id}"))
                .await
                .unwrap()
                .take(0).unwrap();
        Ok(get_data)
    }
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>)//-> Result<(), Error>
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

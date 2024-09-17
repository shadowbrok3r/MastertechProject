use crate::{
    schema::{Record, TaskId, TaskNotePayload, TaskPayload, User, TASK_NOTE_TABLE},
    DATABASE,
};
use anyhow::{Error, Result};
use async_trait::async_trait;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use surrealdb::opt::RecordId;

use super::utilities::Task;

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
        if let Some(id) = id {
            info!("deleting id: {:?}", id.clone());
            DATABASE.set("id", id.0.id.clone()).await?;
            let y: Option<Record> = DATABASE.delete((TASK_NOTE_TABLE, id.0.id)).await?;
            info!("Deleted note: {:?}", y);
        }
        Ok(())
    }
}

pub async fn update_task_notes(new_msg: String, task_id: TaskId) -> Result<(), Error> {
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
        let id: RecordId = self.id.clone().unwrap().0;
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
        let id: RecordId = self.id.clone().unwrap().0;
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
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("SELECT * FROM task_note WHERE id={id}");
        let get_data: Option<T> = DATABASE.query(query).await.unwrap().take(0).unwrap();
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone().unwrap().0;

        let get_data: Option<T> = DATABASE
                .query(format!("SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id={id}"))
                .await
                .unwrap()
                .take(0).unwrap();
        Ok(get_data)
    }
}

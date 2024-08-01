use super::{Priority, Store, TaskNotePayload, TaskPayload, Status, Record};
use anyhow::{Result, Error};
use async_trait::async_trait;
use surrealdb::opt::RecordId;
use crate::DATABASE;
use log::info;

#[async_trait]
pub trait Updatable { 
    async fn update_completed(&self, completed: bool) -> Result<(), Error>;
    async fn update_due_date(&self, due_date: String) -> Result<(), Error>;
    async fn update_assignee_initials(&self, initials: String) -> Result<(), Error>;
    async fn update_task_name(&self, name: String) -> Result<(), Error>;
    async fn update_status(&self, status: Status) -> Result<(), Error>;
    async fn update_dep(&self, store: Store) -> Result<(), Error>;
    async fn update_priority(&self, priority: Option<Priority>) -> Result<(), Error>;
    async fn update_task_description(&self, description: String) -> Result<(), Error>;
    async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> Result<(), Error>;
    async fn update_task_notes(&self, new_msg: String) -> Result<(), Error>;
}

#[async_trait]
impl Updatable for TaskPayload {
    async fn update_completed(&self, completed: bool) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("UPDATE task SET completed=$completed, status=$status WHERE id=$id");
        DATABASE.set("id", id).await?;
        DATABASE.set("completed", completed).await?;
        if completed{ DATABASE.set("status", Status::Complete).await?; }
        else{ DATABASE.set("status", Status::InRepair).await?; }
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }

    async fn update_due_date(&self, due_date: String) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("UPDATE task SET due_date=$date WHERE id=$id");
        DATABASE.set("id", id).await?;
        DATABASE.set("date", due_date).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }

    async fn update_assignee_initials(&self, initials: String) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let user_query = format!("SELECT id FROM user WHERE everest_initials=$initials");
        DATABASE.set("id", id).await?;
        DATABASE.set("initials", initials).await?;    
        let selected_user: Option<Record> = DATABASE.query(user_query).await?.take(0)?;
        let query = format!("UPDATE task SET assignee=$assignee, everest_initials=$initials WHERE id=$id");
        DATABASE.set("assignee", selected_user.unwrap().id).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
        
    }

    async fn update_task_name(&self, name: String) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("UPDATE task SET task_name=$name WHERE id=$id");
        DATABASE.set("id", id).await?;
        DATABASE.set("name", name).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }

    async fn update_status(&self, status: Status) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let mut _query = String::new();
        DATABASE.set("id", id).await?;
        match status{
            Status::Todo => {
                _query = format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
                DATABASE.set("status", Status::Todo).await?;
            },
            Status::InRepair => {
                _query = format!("UPDATE task SET status=$status, completed=false WHERE id=$id");
                DATABASE.set("status", Status::InRepair).await?;
            },
            Status::Complete => {
                _query = format!("UPDATE task SET status=$status, completed=true WHERE id=$id");
                DATABASE.set("status", Status::Complete).await?;
            },
        }

        let _update_task: Vec<Record> = DATABASE.query(_query).await?.take(0)?;
        Ok(())
    }

    async fn update_dep(&self, dep: Store) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("UPDATE task SET dep=$dep WHERE id=$id");
        DATABASE.set("id", id).await?;
        DATABASE.set("dep", dep).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }

    async fn update_priority(&self, priority: Option<Priority>) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("UPDATE task SET priority=$priority WHERE id=$id");
        DATABASE.set("id", id).await?;
        DATABASE.set("priority", priority.unwrap()).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }

    async fn update_task_description(&self, description: String) -> Result<(), Error> {
        let id: RecordId = self.id.clone().unwrap().0;
        let query = format!("UPDATE task SET task_description=$description WHERE id=$id");
        DATABASE.set("id", id).await?;
        DATABASE.set("description", description).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }
    
    async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> Result<(), Error> {
        let id = self.service_ticket.as_ref();
        let x = id.unwrap().id.clone().unwrap().0;
        let query = format!("UPDATE service_order SET checkin_notes=$notes WHERE id=$id");
        DATABASE.set("id", checkin_notes.unwrap()).await?;
        DATABASE.set("notes", x).await?;
        let _update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        Ok(())
    }

    async fn update_task_notes(&self, new_msg: String) -> Result<(), Error> {
        let task_note = TaskNotePayload {
            task_id: self.id.clone(),
            note: new_msg,
            ..Default::default()
        };
        
        let query = format!("CREATE task_note CONTENT $note");
        DATABASE.set("note", task_note).await?;
        let update_task: Vec<Record> = DATABASE.query(query).await?.take(0)?;
        info!("Updated notes: {update_task:?}");
        Ok(())
    }
}

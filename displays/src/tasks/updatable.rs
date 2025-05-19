use database::{schema::{Priority, Record, Status, Store, TaskPayload}, DATABASE};
use async_trait::async_trait;
use surrealdb::RecordId;
use crate::Updatable;
use log::info;


#[async_trait]
impl Updatable for TaskPayload {
    async fn update_service_number(&self, service_number: String) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE
            .query("UPDATE $id SET service_number=$service_number")
            .bind(("id", self.id.clone()))
            .bind(("service_number", service_number))
            .await?
            .take(0)?;
        
        Ok(())
    }

    async fn update_completed(&self, completed: bool) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE
            .query("UPDATE $id SET completed=$completed, status=$status")
            .bind(("id", self.id.clone()))
            .bind(("completed", completed))
            .bind(("status", if completed {Status::Complete} else {Status::InRepair}))
            .await?
            .take(0)?;
        
        Ok(())
    }

    async fn update_due_date(&self) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE
                .query("UPDATE $id SET due_date=$date")
                .bind(("id", self.id.clone()))
                .bind(("date", self.due_date.clone()))
                .await?
                .take(0)?;
        Ok(())
    }

    async fn update_assignee_initials(&self, initials: String) -> anyhow::Result<(), anyhow::Error> {
        info!("Initials: {initials}");
        let selected_user: Option<RecordId> = DATABASE
            .query("SELECT VALUE id FROM user WHERE everest_initials=$initials")
            .bind(("initials", initials.clone()))
            .await?
            .take(0)?;

        info!("Selected user: {selected_user:?}");

        let _update_task: Vec<Record> = DATABASE
            .query("UPDATE $id SET assignee=$assignee, everest_initials=$initials")
            .bind(("id", self.id.clone()))
            .bind(("assignee", selected_user.unwrap()))
            .bind(("initials", initials))
            .await?
            .take(0)?;
        
        Ok(())
    }

    async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE
            .query("UPDATE $id SET task_name=$name")
            .bind(("id", self.id.clone()))
            .bind(("name", name))
            .await?
            .take(0)?;
        
        Ok(())
    }

    async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error> {
        let mut _query = String::new();
        match status {
            Status::Todo => {
                _query = format!("UPDATE $id SET status=$status, completed=false");
                DATABASE.set("status", Status::Todo).await?;
            }
            Status::InRepair => {
                _query = format!("UPDATE $id SET status=$status, completed=false");
                DATABASE.set("status", Status::InRepair).await?;
            }
            Status::Complete => {
                _query = format!("UPDATE $id SET status=$status, completed=true");
                DATABASE.set("status", Status::Complete).await?;
            },
            Status::CustomStatus(status) => {
                _query = format!("UPDATE $id SET status=$status");
                DATABASE.set("status", status).await?;
            }
            _ => {
                _query = format!("UPDATE $id SET status=$status");
                DATABASE.set("status", status).await?;
            }
        }

        let _update_task: Vec<Record> = DATABASE
            .query(_query)
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;
        
        Ok(())
    }

    async fn update_dep(&self, dep: Store) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE
            .query("UPDATE $id SET dep=$dep")
            .bind(("id", self.id.clone()))
            .bind(("dep", dep))
            .await?
            .take(0)?;
        Ok(())
    }

    async fn update_priority(&self, priority: Option<Priority>) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE.query("UPDATE $id SET priority=$priority")
            .bind(("id", self.id.clone()))
            .bind(("priority", priority.unwrap_or_default()))
            .await?
            .take(0)?;
        Ok(())
    }

    async fn update_task_description(&self) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = DATABASE
            .query("UPDATE $id SET task_description=$description")
            .bind(("id", self.id.clone()))
            .bind(("description", self.task_description.clone()))
            .await?
            .take(0)?;
        
        Ok(())
    }

    async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> anyhow::Result<(), anyhow::Error> {
        let ticket = self.service_ticket.as_ref();
        let ticket_id = ticket.cloned().unwrap_or_default().id.clone();

        let _update_task: Vec<Record> = DATABASE.query("UPDATE service_order SET checkin_notes=$notes")
        .bind(("id", checkin_notes.unwrap_or_default()))
        .bind(("notes", ticket_id))
        .await?
        .take(0)?;
        
        Ok(())
    }
}

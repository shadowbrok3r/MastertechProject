use crate::{schema::{LiveTaskPayload, Priority, Record, RecordId, Status}, db};

impl LiveTaskPayload {
    pub async fn update_service_number(&self, service_number: String) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = db()
            .query("UPDATE $id SET service_number=$service_number")
            .bind(("id", self.id.clone()))
            .bind(("service_number", service_number))
            .await?
            .take(0)?;
        
        Ok(())
    }

    pub async fn update_completed(&self, completed: bool) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = db()
            .query("UPDATE $id SET completed=$completed, status=$status")
            .bind(("id", self.id.clone()))
            .bind(("completed", completed))
            .bind(("status", if completed {Status::Complete} else {Status::InRepair}))
            .await?
            .take(0)?;
        
        Ok(())
    }

    pub async fn update_due_date(&self) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = db()
                .query("UPDATE $id SET due_date=$date")
                .bind(("id", self.id.clone()))
                .bind(("date", self.due_date.clone()))
                .await?
                .take(0)?;
        Ok(())
    }

    pub async fn update_assignee(&self, assignee: RecordId) -> anyhow::Result<(), anyhow::Error> {
        log::info!("assignee: {assignee:?}");
        let _update_task: Vec<Record> = db()
            .query("UPDATE $id SET assignee=$assignee, status ='Todo'")
            .bind(("id", self.id.clone()))
            .bind(("assignee", assignee))
            .await?
            .take(0)?;
        
        Ok(())
    }

    pub async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = db()
            .query("UPDATE $id SET task_name=$name")
            .bind(("id", self.id.clone()))
            .bind(("name", name))
            .await?
            .take(0)?;
        
        Ok(())
    }

    pub async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error> {
        let mut _query = String::new();
        match status {
            Status::Todo => {
                _query = format!("UPDATE $id SET status=$status, completed=false");
                db().set("status", Status::Todo).await?;
            }
            Status::InRepair => {
                _query = format!("UPDATE $id SET status=$status, completed=false");
                db().set("status", Status::InRepair).await?;
            }
            Status::Complete => {
                _query = format!("UPDATE $id SET status=$status, completed=true");
                db().set("status", Status::Complete).await?;
            },
            Status::CustomStatus(status) => {
                _query = format!("UPDATE $id SET status=$status");
                db().set("status", status).await?;
            }
            _ => {
                _query = format!("UPDATE $id SET status=$status");
                db().set("status", status).await?;
            }
        }

        let _update_task: Vec<Record> = db()
            .query(_query)
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;
        
        Ok(())
    }

    pub async fn update_priority(&self, priority: Option<Priority>) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = db().query("UPDATE $id SET priority=$priority")
            .bind(("id", self.id.clone()))
            .bind(("priority", priority.unwrap_or_default()))
            .await?
            .take(0)?;
        Ok(())
    }

    pub async fn update_task_description(&self) -> anyhow::Result<(), anyhow::Error> {
        let _update_task: Vec<Record> = db()
            .query("UPDATE $id SET task_description=$description")
            .bind(("id", self.id.clone()))
            .bind(("description", self.task_description.clone()))
            .await?
            .take(0)?;
        
        Ok(())
    }

}
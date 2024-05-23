use log::info;
use serde_json::Value;
use surrealdb::opt::RecordId;
use database::{schema::{ModifyTask, Notification, NotificationStatus, NotificationType, Record, Status, UserId, NOTIFICATION_TABLE}, Database};
use crate::{
    routes::user::query_user_from_initials, utils::error::Error
};


pub async fn update_task(db: Database, payload: ModifyTask) 
-> Result<Vec<Value>, Error>{
    let id: RecordId = payload.clone().task_id.0;
    let query: String;

    info!("payload: {:?}", payload.clone());
    if let Some(completion) = payload.completed{
        if completion {
            query = format!(
                "UPDATE task SET completed=true, status='{:?}' WHERE id = {id}",
                Status::Complete,
            );
        }else {
            query = format!(
                "UPDATE task SET completed=false, status='{:?}' WHERE id = {id}", 
                Status::InRepair, 
            );
        }
    }else if let Some(status) = payload.status{ 
        match status{
            Status::Todo => {
                query = format!(
                    "UPDATE task SET status='{status:?}', completed=false WHERE id={id}"
                );
            },
            Status::InRepair => {
                query = format!(
                    "UPDATE task SET status='{status:?}', completed=false WHERE id={id}"
                );
            },

            Status::Complete => {
                query = format!(
                    "UPDATE task SET status='{status:?}', completed=true WHERE id={id}"
                );
            },
        }
    }else if let Some(due_date) = payload.due_date{ 
            query = format!("UPDATE task SET due_date='{due_date}' WHERE id={id}");          
    }else if let Some(task_description) = payload.task_description{ 
        query = format!("UPDATE task SET task_description='{task_description}' WHERE id={id}");          
    }else if let Some(assignee_initials) = payload.assignee_initials.clone(){ 
            let usr: RecordId = query_user_from_initials(db.clone(), Some(assignee_initials.clone()), None)
                .await.unwrap_or(None).unwrap().0;
            // debug!("assignee_initials: {}", assignee_initials.clone());
            // debug!("usr: {}", usr.clone());
            let notification = Notification{
                user: UserId(usr.clone()),
                notification_description: "New Task Assigned To You".to_string(),
                notification_type: NotificationType::NewTask,
                status: NotificationStatus::Unread,
                user_initials: assignee_initials.clone(),
            };

            let _: Vec<Record> = db
                .database
                .create(NOTIFICATION_TABLE)
                .content(notification)
                .await?;
            
            query = format!("UPDATE task SET assignee={usr}, assignee_initials='{assignee_initials}' WHERE id={id}");

    }else if let Some(task_name) = payload.task_name{ 
            query = format!("UPDATE task SET task_name={task_name} WHERE id={id}"); 
    }else if let Some(priority) = payload.priority{ 
            query = format!("UPDATE task SET priority='{priority:?}' WHERE id={id}");          
    }else { query = String::new()}

    info!("query: {:?}", query.clone());

    let update_task: Vec<Value> = db
        .database
        .query(query)
        .await?
        .take(0).unwrap();

    info!("update_task: {:?}", update_task.clone());

    Ok(update_task)
}

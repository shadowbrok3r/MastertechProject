use crate::{ routes::user::query_user_from_initials, utils::error::Error };
use database::{schema::{ModifyNotification, NotificationStatus, UserId}, Database};
use log::debug;
use serde_json::Value;
use surrealdb::opt::RecordId;

pub async fn update_notification(db: Database, payload: ModifyNotification) 
-> Result<Vec<Value>, Error>{
    debug!("Payload: {:?}", payload.clone());
    let id: RecordId = payload.clone().id.0;
    let query: String;
    // I think i just need to pass this an enum..
    // maybe have a required enum of what we are actually 
    // trying to accomplish along with the existing data?
    
    let queried_user: Option<UserId> = query_user_from_initials(
        db.clone(), 
        payload.everest_initials.clone(),
        None
    ).await.unwrap_or(None);
    
    if let Some(status) = payload.status{
        match status {
            NotificationStatus::Read => {
                query = format!(
                    "UPDATE notification SET status='{:?}' WHERE id = {id}",
                    NotificationStatus::Read,
                );
            },
            NotificationStatus::Unread => {
                query = format!(
                    "UPDATE notification SET status='{:?}' WHERE id = {id}",
                    NotificationStatus::Unread,
                );
            },
        } 
    }else if let Some(mark_unread) = payload.mark_all_unread{
        if mark_unread == true{
            if let Some(user) = queried_user{
                query = format!(
                    "FOR $notifications IN (SELECT * FROM notification WHERE everest_initials='{}'){{ UPDATE $notifications SET status='Unread' }}",
                    user.0
                );
            }else{query = String::new();}

        }else {query = String::new();}

    }else if let Some(mark_read) = payload.mark_all_read{
        if mark_read == true{
            if let Some(user) = queried_user{
                query = format!(
                    "FOR $notifications IN (SELECT * FROM notification WHERE everest_initials='{}'){{ UPDATE $notifications SET status='Read' }}",
                    user.0
                );
            }else{query = String::new();}
        }else{query = String::new();}
    }else if let Some(archive) = payload.archive{
        query = format!("UPDATE notification SET archive='{archive}' WHERE id = {id}");
    }else { query = String::new()}

    let update_notification: Vec<Value> = db
        .database
        .query(query)
        .await?
        .take(0)?;

    Ok(update_notification)
}

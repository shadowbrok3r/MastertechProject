
use log::debug;
use surrealdb::opt::RecordId;
use tokio::sync::mpsc;
use database::{schema::{Record, TaskNotePayload, TASK_NOTE_TABLE, Notification, NOTIFICATION_TABLE, NotificationStatus, NotificationType, UserId}, Database};
use crate::utils::error::Error;
use crate::user::query_user_from_initials;

pub async fn insert_task_note(db: Database, payload: TaskNotePayload) 
-> Result<Vec<Record>, Error>{

    let usr: RecordId = query_user_from_initials(db.clone(), Some(payload.everest_initials.clone()), None)
        .await.unwrap_or(None).unwrap().0;

    let task_note = TaskNotePayload{
        created_at: payload.created_at,
        // service_number: payload.service_number,
        note: payload.note,
        everest_initials: payload.everest_initials.clone(),
        task_id: payload.task_id.clone(),
    };

    let notification = Notification{
        user: UserId(usr.clone()),
        notification_description: "You have a new message!".to_string(),
        notification_type: NotificationType::NewMessage,
        status: NotificationStatus::Unread,
        user_initials: payload.everest_initials,
    };


    let (tx, mut rx) = mpsc::channel::<Vec<Record>>(1);

    let sender = tx.clone();

    tokio::spawn(async move{

        let note_records: Vec<Record>  = db
            .database
            .create(TASK_NOTE_TABLE)
            .content(task_note)
            .await.unwrap();

        match sender.send(note_records).await{
            Ok(_) => {
                debug!("Sent note_records");
            },
            Err(err) => {
                debug!("Error sending note_records, {err:?}");
            },
        };

        let create_notification: Vec<Record>  = db
            .database
            .create(NOTIFICATION_TABLE)
            .content(notification)
            .await.unwrap();

            debug!("{create_notification:?}");
        /* 
        match sender.send(create_notification).await{
            Ok(x) => {
                debug!("Sent create_notification");
            },
            Err(err) => {
                debug!("Error sending create_notification");
            },
        }; 
        */

        drop(sender);
    });

    drop(tx);

    let x = rx.recv().await;

    // debug!("create rec: {:?}", x.unwrap().clone());

    Ok(x.unwrap())
}
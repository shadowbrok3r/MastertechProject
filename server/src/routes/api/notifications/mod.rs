use axum::{Extension, Json};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::{
    middlewares::context::Ctx, routes::user::query_user_from_initials, utils::error::{ApiError, Error}
};
use database::{schema::{ModifyNotification, UserId} , Database};
use self::update::update_notification;
pub mod update;

#[derive(Debug, Deserialize, Serialize)]
pub struct UserNotifications {
    everest_initials: String 
}

pub async fn handle_get_notifications(
    db: Extension<Database>, 
    ctx: Ctx,
    Json(payload): Json<UserNotifications>
) -> Json<Result<Vec<Value>, ApiError>> { 
    let queried_user: Option<UserId> = query_user_from_initials(
        db.0.clone(), 
        Some(payload.everest_initials.clone()),
        None
    ).await.unwrap();


    let notifications: Result<Vec<Value>, ApiError> = db
        .database
        .query(format!("SELECT * FROM notification WHERE user = {}", queried_user.unwrap().0))
        .await
        .or_else(|err|{
            Err(ApiError{
                error: Error::Generic { description: err.to_string() },
                req_id: ctx.req_id()
            })
        })
        .unwrap()
        .take(0).or_else(|err| {
            Err(ApiError{
                error: Error::Generic { description: err.to_string() },
                req_id: ctx.req_id()
            })
        });

    debug!("notifications: {notifications:?}");
    Json(notifications)
}

pub async fn handle_notification_modification(
    db: Extension<Database>, 
    ctx: Ctx,
    Json(payload): Json<ModifyNotification>
) -> Json<Result<Vec<Value>, ApiError>>{
    // println!("data in update task: {:#?}", payload);

    let update_ticket = update_notification(db.0, payload)
        .await;

    let res = update_ticket
        .or_else(|err|{
            Err(ApiError{
                error: Error::Generic { description: err.to_string() },
                req_id: ctx.req_id()
            })
        }).and_then(|response|{
            Ok(response)
        });

    Json(res)
}
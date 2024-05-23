use axum::{Extension, Json};
use log::debug;
use serde_json::Value;
use surrealdb::sql::Thing;
use database::{schema::{ModifyTask, Record, TaskPayload, TicketId, UserId, TICKET_TABLE }, Database};
use crate::{
    middlewares::context::Ctx, routes::{api::tasks::create::insert_task, user::query_user_from_initials}, utils::error::{
        ApiError,
        Error
    }
};

use self::{update::update_task, get::query_service_tasks};
pub mod create;
pub mod get;
pub mod update;
pub mod delete;


pub async fn handle_create_task(
    db: Extension<Database>, 
    ctx: Ctx,
    Json(payload): Json<TaskPayload>
) -> Json<Result<Vec<Record>, ApiError>> { 
    let pld = &payload;
    debug!("payload: {pld:?}");

    // let task_id = TaskId(Thing::from((TASK_TABLE.to_string(), format!("{}-{}", payload.service_number, Uuid::new_v4()))));
    let ticket_id: Option<TicketId>;
    
    if let Some(service_number) = payload.service_number{
        ticket_id = Some(TicketId(Thing::from((TICKET_TABLE.to_string(), service_number.to_string()))));
    }else{
        ticket_id = None;
    }

    let queried_user: Option<UserId> = query_user_from_initials(
        db.0.clone(), 
        payload.assignee_initials.clone(),
        payload.assignee_email.clone()
    ).await.unwrap();

    let task = TaskPayload{
        id: None,
        service_ticket: ticket_id,
        task_description: payload.task_description, 
        assignee_email: payload.assignee_email,
        assignee_initials: payload.assignee_initials,
        assignee: queried_user,
        service_number: payload.service_number,
        due_date: payload.due_date,
        task_note: None, //payload.task_note
        completed: false,
        priority: payload.priority,
        task_name: payload.task_name,
        status: payload.status,
        dep: payload.dep,
    };

    let insert_data = insert_task(db.0, task)
        .await;

    let res = insert_data
        .or_else(|err|{
            Err(ApiError{
                error: Error::Generic { description: err.to_string() },
                req_id: ctx.req_id()
            })
        }).and_then(|rec|{
            Ok(rec)
        });

    Json(res)
}


pub async fn handle_task_modification(
    db: Extension<Database>, 
    ctx: Ctx,
    Json(payload): Json<ModifyTask>
) -> Json<Result<Vec<Value>, ApiError>>{
    // println!("data in update task: {:#?}", payload);

    let update_ticket = update_task(db.0, payload)
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



pub async fn handle_get_tasks(
    db: Extension<Database>,
    _ctx: Ctx,
    // Json(service_number): Json<i32>
) -> Json<Result<Vec<Value>, Error>> {
    let tasks: Result<Vec<Value>, Error> = query_service_tasks(db.0)
        .await;
    Json(tasks)
}

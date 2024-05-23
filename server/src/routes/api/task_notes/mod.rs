use axum::{Extension, Json};
use log::{debug, warn};
use database::{schema::{RecordResult, TaskNotePayload}, Database};
use crate::{
    middlewares::context::Ctx, routes::api::task_notes::{create::insert_task_note, update::update_task_with_note}, utils::error::{
        ApiError,
        Error
    }
};

pub mod create;
pub mod update;
pub mod delete;

pub async fn handle_create_task_note(
    db: Extension<Database>, 
    ctx: Ctx,
    Json(payload): Json<TaskNotePayload>
) -> Json<Result<RecordResult, ApiError>> { 
    // let task_note_id = TaskNoteId(Thing::from((TASK_NOTE_TABLE.to_string(), payload.service_number.to_string())));
   
    let insert_data = insert_task_note(db.0.clone(), payload.clone())
        .await;

    if let Ok(records) = &insert_data{
        debug!("record ok, updating task: {records:#?}");
        let _ = update_task_with_note(db.0.clone(), records, payload.task_id.unwrap())
            .await.unwrap();
    }

    let res = insert_data
        .or_else(|err|{
            Err(ApiError{
                error: Error::Generic { description: err.to_string() },
                req_id: ctx.req_id()
            })
        }).and_then(|rec|{
            let x = format!("Returned records: {rec:#?}");
            Ok(RecordResult{
                result: true,
                record: Some(x)
            })
        });

        warn!("task_note_data => {:?}", res);

    Json(res)
}


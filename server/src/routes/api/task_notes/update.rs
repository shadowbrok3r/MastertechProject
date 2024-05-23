use log::debug;
use surrealdb::Response;
use database::{Database, schema::{Record, TaskId}};
use crate::utils::error::Error;

pub async fn update_task_with_note(db: Database, records: &Vec<Record>, task_id: TaskId) 
-> Result<Response, Error>{
    let rec = if let Some(record) = records.iter().next().take(){
        Some(record.id.clone())
    }else{
        None
    };

    let query = format!( "UPDATE task SET task_note += [{}] WHERE id = {}", rec.unwrap(), task_id.0);
    debug!("query: {}", query.clone());

    let update_task: Response = db
        .database
        .query(query)
        .await?;

    debug!("Updated task: {update_task:?}");
    Ok(update_task)
}
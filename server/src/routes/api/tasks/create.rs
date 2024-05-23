use database::{schema::{Record, TaskPayload, TASK_TABLE}, Database};
use crate::utils::error::Error;


pub async fn insert_task(db: Database, task_payload: TaskPayload) 
-> Result<Vec<Record>, Error>{

    let create_record: Vec<Record> = db
        .database
        .create(TASK_TABLE)
        .content(task_payload)
        .await?;

    println!("create rec: {create_record:?}");

    Ok(create_record)
}
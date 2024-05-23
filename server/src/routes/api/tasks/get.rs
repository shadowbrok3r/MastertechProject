use serde_json::Value;
use database::Database;
use crate::utils::error::Error;


pub async fn query_service_tasks(db: Database)
-> Result<Vec<Value>, Error>{ // Vec<TicketData>
    let task_data: Vec<Value> = db
        .database
        .query( // WHERE service_number = {}
            "SELECT * FROM task  ORDER BY due_date DESC FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note"
        )
        .await?
        .take(0)?;
    println!("task_data: {}", task_data[0]);
    Ok(task_data)
}
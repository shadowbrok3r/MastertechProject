use log::debug;
use serde_json::Value;
use database::Database;
use crate::utils::error::Error;

pub async fn query_ticket(db: Database)
-> Result<Vec<Value>, Error>{ // Vec<TicketData>
    let mut query = db
        .database
        .query("SELECT * FROM service_order FETCH customer, computer") // 
        .await?;

        
    let y = query.take_errors();
    
    debug!("errors: {y:?}");

    let ticket_data: Vec<Value> = query // : Vec<TicketPayload>
        .take(0)?;

    Ok(ticket_data)
}
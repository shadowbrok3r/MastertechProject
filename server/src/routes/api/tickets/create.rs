
use database::{schema::{ComputerData, CustomerData, Record, TaskPayload, TicketData, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE}, Database};
use crate::utils::error::Error;

pub async fn insert_ticket(
    db: Database, 
    ticket_payload: TicketData, 
    customer_payload: CustomerData,
    computer_payload: ComputerData,
    task_payload: TaskPayload
) 
-> Result<Vec<Record>, Error>{

    let create_record: Vec<Record> = db
        .database
        .create(TICKET_TABLE)
        .content(ticket_payload)
        .await?;

    println!("create_rec: {create_record:?}");

    let create_cust_record: Vec<Record> = db
        .database
        .create(CUSTOMER_TABLE)
        .content(customer_payload)
        .await?;

    println!("create_cust_record: {create_cust_record:?}");

    let create_computer_record: Vec<Record> = db
        .database
        .create(COMPUTER_TABLE)
        .content(computer_payload)
        .await?;

    println!("create_computer_record: {create_computer_record:?}");

    let create_task_record: Vec<Record> = db
        .database
        .create(TASK_TABLE)
        .content(task_payload)
        .await.unwrap();

    println!("create_task_record: {create_task_record:?}");

    Ok(create_task_record)
}

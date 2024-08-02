use database::{schema::{utilities::{query_id, query_user_from_email}, ComputerData, CustomerData, LiveTaskPayload, Priority, Record, TaskNotePayload, TicketData, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE}, DATABASE};
use crate::app_state::MastertechContext;
use chrono::{DateTime, SecondsFormat};
use log::info;
use tokio::spawn;

impl MastertechContext{
    pub fn submit_tur_mastertech(&mut self) {
        let due_date = Some(
            self.date.unwrap_or(DateTime::default()).to_rfc3339_opts(SecondsFormat::Secs,  true)
        );
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();
        
        task_data.due_date = due_date.unwrap_or_default();

        spawn(async move {
            let x = send_payload(
                ticket_data,
                customer_data,
                computer_data,
                task_data,
                task_notes
            ).await;
            info!("output: {x:?}");
        });
    }

}

pub async fn send_payload(
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    mut task_notes: Vec<TaskNotePayload>,
) -> anyhow::Result<Vec<Record>, anyhow::Error> {
    info!("Send_Payload");
    let queried_salesman = query_user_from_email(ticket_data.salesman.clone()).await?;
    let _queried_tech = query_user_from_email(ticket_data.tech.clone()).await?;
    
    let task_id = task_data.id.clone();
    let ticket_id = ticket_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();

    task_data.task_name = format!("{} - {}", &customer_data.name, ticket_data.service_number.clone());
    task_data.service_ticket = ticket_id.clone();
    task_data.service_number = Some(ticket_data.service_number.clone());
    task_data.priority = Priority::Normal;
    task_data.everest_initials = queried_salesman.everest_initials;
    task_data.assignee = queried_salesman.id;

    if let Some(cust) = query_id(CUSTOMER_TABLE, customer_id).await?{
        let update_cust_record: Option<Record> = DATABASE.update(cust.id).content(customer_data.clone()).await?;
        info!("Customer updated: {update_cust_record:?}");

        if let Some(computer_record) = query_id(COMPUTER_TABLE, computer_id).await?{
            let create_computer_record: Option<Record> = DATABASE.update(computer_record.id).content(computer_data).await?;
            info!("create_computer_record: {create_computer_record:?}");
        }else{
            let create_computer_record: Vec<Record> = DATABASE.create(COMPUTER_TABLE).content(computer_data).await?;
            info!("create_computer_record: {create_computer_record:?}");
        }
        if let Some(ticket) = query_id(TICKET_TABLE, ticket_id).await?{
            let service_ticket_record: Option<Record> = DATABASE.update(ticket.id).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }else{
            let service_ticket_record: Vec<Record> = DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }
    } else {
        let create_cust_record: Vec<Record> = DATABASE.create(CUSTOMER_TABLE).content(customer_data.clone()).await?;
        info!("create_cust_record created: {create_cust_record:?}");
        let create_computer_record: Vec<Record> = DATABASE.create(COMPUTER_TABLE).content(computer_data).await?;
        info!("create_computer_record created: {create_computer_record:?}");
        let service_ticket_record: Vec<Record> = DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
        info!("service_ticket_record created: {service_ticket_record:?}");
    }

    let create_task_record: Vec<Record> = DATABASE.create(TASK_TABLE).content(task_data).await?;
    info!("create_task_record: {create_task_record:?}");

    if task_notes.len() > 0 {
        info!("Task Notes: {:?}", task_notes);
        for note in task_notes.iter_mut() {
            note.task_id = task_id.clone();
            let create_task_note_record: Vec<Record> = DATABASE.create(TASK_NOTE_TABLE).content(note).await?;
            info!("create_task_note_record: {:?}", create_task_note_record);

            if let (Some(record), Some(note_record)) = (create_task_record.get(0), create_task_note_record.get(0)) {
                let update_task: Option<Record> = DATABASE.query("UPDATE $task SET task_note += $note")
                    .bind(("task", record.id.clone()))
                    .bind(("note", note_record))
                    .await?.take(0)?;
                
                info!("Update_task with notes: {update_task:?}");
            }
        }
    }
    
    Ok(create_task_record)
}
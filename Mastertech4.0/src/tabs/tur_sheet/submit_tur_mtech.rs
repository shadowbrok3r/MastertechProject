use crate::app_state::MastertechContext;
use chrono::{DateTime, SecondsFormat};
use database::{
    schema::{
        helper_traits::TaskNotePayloadHelper, 
        utilities::{query_id, query_user_from_email}, 
        ComputerData, CustomerData, LiveTaskPayload, Priority, Record, TaskNotePayload, TicketData, COMPUTER_TABLE, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE
    },
    DATABASE,
};
use log::{error, info};
use surrealdb::RecordId;
use tokio::spawn;

impl MastertechContext {
    pub fn submit_tur_mastertech(&mut self) {
        let due_date = Some(
            self.date
                .unwrap_or(DateTime::default())
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        );
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        task_data.due_date = due_date.unwrap_or_default();
        let send_specs = self.send_specs.clone();
        spawn(async move {
            let send_payload_result = send_payload(
                ticket_data,
                customer_data,
                computer_data,
                task_data,
                task_notes,
                send_specs,
            )
            .await;
            info!("send_payload_result: {send_payload_result:?}");
        });
    }
}

pub async fn send_payload(
    ticket_data: TicketData,
    customer_data: CustomerData,
    computer_data: ComputerData,
    mut task_data: LiveTaskPayload,
    task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
) -> anyhow::Result<(), anyhow::Error> {
    info!("Send_Payload");
    let queried_salesman = query_user_from_email(ticket_data.salesman.clone()).await?;
    let _queried_tech = query_user_from_email(ticket_data.tech.clone()).await?;

    // let task_id = task_data.id.clone();
    let ticket_id = ticket_data.id.clone();
    let customer_id = customer_data.id.clone();
    let computer_id = computer_data.id.clone();

    task_data.task_name = format!(
        "{} - {}",
        &customer_data.name,
        ticket_data.service_number.clone()
    );
    task_data.service_ticket = Some(ticket_id.clone());
    task_data.service_number = Some(ticket_data.service_number.clone());
    task_data.priority = Priority::Normal;
    task_data.everest_initials = queried_salesman.everest_initials;
    task_data.assignee = queried_salesman.id;

    if let Some(cust) = query_id(CUSTOMER_TABLE.to_string(), customer_id).await? {
        let update_cust_record: Vec<RecordId> = DATABASE
            .update(cust.key().to_string())
            .content(customer_data.clone())
            .await?;
        info!("Customer updated: {update_cust_record:?}");

        if let Some(computer_record) = query_id(COMPUTER_TABLE.to_string(), computer_id).await? {
            if send_specs {
                let create_computer_record: Vec<RecordId> = DATABASE
                    .update(computer_record.key().to_string())
                    .content(computer_data)
                    .await?;
                info!("create_computer_record: {create_computer_record:?}");
            }
        } else {
            let create_computer_record: Option<RecordId> = DATABASE
                .create(COMPUTER_TABLE)
                .content(computer_data)
                .await?;
            info!("create_computer_record: {create_computer_record:?}");
        }
        if let Some(ticket) = query_id(TICKET_TABLE.to_string(), ticket_id).await? {
            let service_ticket_record: Vec<RecordId> = DATABASE
                .update(ticket.key().to_string())
                .content(ticket_data)
                .await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        } else {
            let service_ticket_record: Option<RecordId> =
                DATABASE.create(TICKET_TABLE).content(ticket_data).await?;
            info!("service_ticket_record: {service_ticket_record:?}");
        }
    } else {
        match DATABASE
            .create::<Option<Record>>(CUSTOMER_TABLE)
            .content(customer_data.clone())
            .await
        {
            Ok(create_cust_record) => info!("Created Record: {create_cust_record:?}"),
            Err(e) => error!("Error with create_cust_record: {e:?}"),
        }
        if send_specs {
            match DATABASE
                .create::<Option<Record>>(COMPUTER_TABLE)
                .content(computer_data)
                .await
            {
                Ok(create_computer_record) => info!("Created Record: {create_computer_record:?}"),
                Err(e) => error!("Error with create_computer_record: {e:?}"),
            }
        }
        match DATABASE
            .create::<Option<Record>>(TICKET_TABLE)
            .content(ticket_data)
            .await
        {
            Ok(create_ticket_record) => info!("Created Record: {create_ticket_record:?}"),
            Err(e) => error!("Error with create_ticket_record: {e:?}"),
        }
    }

    info!("Task Data: {:?}", &task_data);

    let create_task_record: Option<Record> = DATABASE
        .create(TASK_TABLE)
        .content(task_data).await?;

    info!("create_task_record: {create_task_record:?}");

    for mut note in task_notes {
        let res = note.create_task_note().await;
        info!("Task Note Creation from Mastertech: {res:?}");
    }
    Ok(())
}

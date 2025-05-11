use log::Record;

use crate::schema::{helper_traits::TaskNotePayloadHelper, TASK_TABLE};

use super::{ComputerData, CustomerData, LiveTaskPayload, TaskNotePayload, TaskPayload, TicketData};

impl TaskPayload {
    pub fn from_prestashop_payload(data: PrestashopPayload) -> Self {

        Self {
            id: RecordId,
            task_name: String,
            service_ticket: Option<TicketPayload>,
            everest_initials: String,
            task_description: String,
            assignee: RecordId,
            service_number: Option<String>,
            due_date: String,
            priority: Priority,
            task_note: Vec<TaskNotePayload>,
            completed: bool,
            status: Status,
            created_at: Option<String>
        }
    }

    pub async fn create_task_payload(
        mut task_data: Self,
        ticket_data: TicketData,
        customer_data: CustomerData,
        computer_data: ComputerData,
        // mut task_data: LiveTaskPayload,
        mut task_notes: Vec<TaskNotePayload>,
        send_specs: bool,
    ) -> anyhow::Result<(), anyhow::Error> {
        // let mut task_data = self;
        info!("schema/utilities.rs -> Send_Payload");
        let queried_salesman = query_user_from_email(ticket_data.salesman.clone()).await.unwrap_or_default();
        let _queried_tech = query_user_from_email(ticket_data.tech.clone()).await.unwrap_or_default();
        
        
        // let task_id = task_data.id.clone();
        let ticket_id = ticket_data.id.clone();
        let customer_id = customer_data.id.clone();
        let computer_id = computer_data.id.clone();
        let service_number = ticket_data.service_number.clone();
        task_data.task_name = format!(
            "{} - {}",
            &customer_data.name,
            service_number.clone()
        );
        task_data.service_ticket = Some(ticket_id.clone());
        task_data.service_number = Some(service_number.clone());
        task_data.priority = Priority::Normal;
        task_data.everest_initials = queried_salesman.everest_initials;
        task_data.assignee = queried_salesman.id;
    
        // if ticket_data.computer.is_none() {
        //     ticket_data.computer = Some(computer_data.id.clone());
        // }
    
        info!("schema/utilities.rs -> cust_record: {customer_data:?}");
        let update_customer: Result<Option<Record>, surrealdb::Error> = DATABASE
            .upsert(customer_id)
            .content(customer_data.clone())
            .await;
        
        match update_customer {
            Ok(record) => log::info!("Updated Customer {record:?}"),
            Err(e) => {
                log::warn!("Error updating Customer {e:?}");
                // if i have a customer from everest, i will need to delete
                // and recreate the record.. 
            }
        }
    
        // panic!("");
        if send_specs {
            let create_computer_record: Option<Record> = DATABASE
                .upsert(computer_id)
                .content(computer_data)
                .await?;
            info!("schema/utilities.rs -> create_computer_record: {create_computer_record:?}");
        }
    
        info!("schema/utilities.rs -> ticket record: {ticket_data:?}");
        let service_ticket_record: Option<Record> = DATABASE
            .upsert(ticket_id)
            .content(ticket_data)
            .await?;
        info!("schema/utilities.rs -> service_ticket_record: {service_ticket_record:?}");
    
        info!("schema/utilities.rs -> Task Data: {:?}", &task_data);
    
        
        let check_task_record: Vec<LiveTaskPayload> = DATABASE
            .query("SELECT * FROM task WHERE service_number == $service_number")
            .bind(("service_number", service_number.clone()))
            .await?
            .take(0)?;
    
        info!("schema/utilities.rs -> check_task_record: {check_task_record:?}");
    
        if !check_task_record.is_empty() {
            for task in check_task_record.iter() {
                if task.id == task_data.id {
                    let upsert_task_record: Option<Record> = DATABASE
                        .update(task.id.clone())
                        .content(LiveTaskPayload {
                            id: task.id.clone(),
                            ..task_data.clone()
                        }).await?;
    
                    for note in task_notes.iter_mut() {
                        if note.task_id == Some(task_data.id.clone()) && note.task_id != Some(task.id.clone()) {
                            note.task_id = Some(task.id.clone());
                        }
                    }
                    info!("schema/utilities.rs -> upsert_task_record: {upsert_task_record:?}");
                }
    
            } 
        } else {
            let create_task_record: Option<Record> = DATABASE
                .create(TASK_TABLE)
                .content(task_data).await?;
            info!("schema/utilities.rs -> create_task_record: {create_task_record:?}");
        }
    
        for mut note in task_notes {
            let res = note.handle_note_creation(false).await;
            info!("schema/utilities.rs -> Task Note Creation from Mastertech: {res:?}");
        }
    
        Ok(())
    }
}
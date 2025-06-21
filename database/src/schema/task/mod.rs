use crate::{schema::{Priority, Record, User, TASK_TABLE}, DATABASE};
use structdiff::{Difference, StructDiff};
use surrealdb::{sql::Datetime, RecordId};
use chrono::Utc;

use super::{ComputerData, CustomerData, Status, TaskNotePayload, TicketData, TicketPayload, USER_TABLE};

pub mod update;
pub mod sort;
pub mod filter;

pub use filter::*;
pub use sort::*;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TaskPayload {
    pub id: RecordId,
    pub task_name: String,
    pub service_ticket: Option<TicketPayload>,
    pub task_description: String,
    pub assignee: RecordId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: Datetime, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    #[difference(collection_strategy = "ordered_array_like")]
    pub task_note: Vec<TaskNotePayload>,
    pub completed: bool,
    pub status: Status,
    pub created_at: Datetime
}

impl Default for TaskPayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TASK_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))),
            task_name: String::new(),
            service_ticket: None,
            task_description: String::new(),
            assignee: RecordId::from((USER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))),
            service_number: None,
            due_date: Utc::now().into(),
            priority: Priority::Normal,
            task_note: Vec::new(),
            completed: false,
            status: Status::Todo,
            created_at: Utc::now().into()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct LiveTaskPayload {
    pub id: RecordId,
    pub task_name: String,
    pub service_ticket: Option<RecordId>,
    pub task_description: String,
    pub assignee: RecordId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: Datetime, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    pub completed: bool,
    pub status: Status,
    pub created_at: Datetime
}

impl Default for LiveTaskPayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TASK_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))),
            task_name: String::new(),
            service_ticket: None,
            task_description: String::new(),
            assignee: RecordId::from((USER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))),
            service_number: None,
            due_date: Utc::now().into(),
            priority: Priority::Normal,
            completed: false,
            status: Status::Todo,
            created_at: Utc::now().into()
        }
    }
}

impl TaskPayload {
    // pub fn into_live_task_payload(&self) -> &mut LiveTaskPayload {
    //     let task = &mut LiveTaskPayload {
    //         id: self.id,
    //         task_name: self.task_name,
    //         service_ticket: Some(self.service_ticket.unwrap_or_default().id),
    //         task_description: self.task_description,
    //         assignee: self.assignee,
    //         service_number: self.service_number,
    //         due_date: self.due_date,
    //         priority: self.priority,
    //         completed: self.completed,
    //         status: self.status,
    //         created_at: self.created_at
    //     };

    //     task
    // }
}

impl LiveTaskPayload {
    pub async fn get_associated_computer(&self) -> anyhow::Result<ComputerData, anyhow::Error> {
        let computer: Option<ComputerData> = DATABASE
            .query("SELECT service_ticket FROM $id FETCH service_ticket.computer")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(computer.unwrap_or_default())
    }

    pub async fn get_associated_service(&self) -> anyhow::Result<TicketData, anyhow::Error> {
        let ticket: Option<TicketData> = DATABASE
            .query("SELECT service_ticket FROM $id FETCH service_ticket")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(ticket.unwrap_or_default())
    }

    pub async fn get_associated_customer(&self) -> anyhow::Result<CustomerData, anyhow::Error> {
        let customer: Option<CustomerData> = DATABASE
            .query("SELECT service_ticket FROM $id FETCH service_ticket.customer")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(customer.unwrap_or_default())
    }

    pub async fn get_associated_notes(&self) -> anyhow::Result<Vec<TaskNotePayload>, anyhow::Error> {
        let notes: Vec<TaskNotePayload> = DATABASE
            .query("SELECT * FROM task_note WHERE task_id == $id")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(notes)
    }

    pub async fn get_tasks(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let tasks: Vec<Self> = DATABASE
            .query("SELECT * FROM task ORDER BY due_date DESC START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(tasks)
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
        log::info!("schema/utilities.rs -> Send_Payload");
        let queried_salesman = User::query_user_from_email(ticket_data.salesman.clone()).await.unwrap_or_default();
        let _queried_tech = User::query_user_from_email(ticket_data.tech.clone()).await.unwrap_or_default();
        
        
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
        task_data.assignee = queried_salesman.get_id();
    
        // if ticket_data.computer.is_none() {
        //     ticket_data.computer = Some(computer_data.id.clone());
        // }
    
        log::info!("schema/utilities.rs -> cust_record: {customer_data:?}");
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
            log::info!("schema/utilities.rs -> create_computer_record: {create_computer_record:?}");
        }
    
        log::info!("schema/utilities.rs -> ticket record: {ticket_data:?}");
        let service_ticket_record: Option<Record> = DATABASE
            .upsert(ticket_id)
            .content(ticket_data)
            .await?;
        log::info!("schema/utilities.rs -> service_ticket_record: {service_ticket_record:?}");
    
        log::info!("schema/utilities.rs -> Task Data: {:?}", &task_data);
    
        
        let check_task_record: Vec<LiveTaskPayload> = DATABASE
            .query("SELECT * FROM task WHERE service_number == $service_number")
            .bind(("service_number", service_number.clone()))
            .await?
            .take(0)?;
    
        log::info!("schema/utilities.rs -> check_task_record: {check_task_record:?}");
    
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
                    log::info!("schema/utilities.rs -> upsert_task_record: {upsert_task_record:?}");
                }
    
            } 
        } else {
            let create_task_record: Option<Record> = DATABASE
                .create(TASK_TABLE)
                .content(task_data).await?;
            log::info!("schema/utilities.rs -> create_task_record: {create_task_record:?}");
        }
    
        for mut note in task_notes {
            let res = note.handle_note_creation().await;
            log::info!("schema/utilities.rs -> Task Note Creation from Mastertech: {res:?}");
        }
    
        Ok(())
    }

    pub fn from_task_payload(task: &mut TaskPayload) -> &mut LiveTaskPayload {
        let task = &mut LiveTaskPayload {
            id: task.id,
            task_name: task.task_name,
            service_ticket: Some(task.service_ticket.unwrap_or_default().id),
            task_description: task.task_description,
            assignee: task.assignee,
            service_number: task.service_number,
            due_date: task.due_date,
            priority: task.priority,
            completed: task.completed,
            status: task.status,
            created_at: task.created_at
        };

        task
    }
}

impl From<LiveTaskPayload> for TaskPayload {
    fn from(live_task: LiveTaskPayload) -> Self {
        Self {
            id: live_task.id,
            task_name: live_task.task_name,
            task_description: live_task.task_description,
            assignee: live_task.assignee,
            service_number: live_task.service_number,
            due_date: live_task.due_date,
            priority: live_task.priority,
            completed: live_task.completed,
            status: live_task.status,
            ..Default::default()
        }
    }
}

impl From<TaskPayload> for LiveTaskPayload {
    fn from(task: TaskPayload) -> Self {
        Self {
            id: task.id,
            task_name: task.task_name,
            service_ticket: Some(task.service_ticket.unwrap_or_default().id),
            task_description: task.task_description,
            assignee: task.assignee,
            service_number: task.service_number,
            due_date: task.due_date,
            priority: task.priority,
            completed: task.completed,
            status: task.status,
            created_at: task.created_at
        }
    }
}

// impl From<&mut TaskPayload> for &mut LiveTaskPayload {
//     fn from(task: &mut TaskPayload) -> &mut Self {
//         &mut Self {
//             id: task.id,
//             task_name: task.task_name,
//             service_ticket: Some(task.service_ticket.unwrap_or_default().id),
//             task_description: task.task_description,
//             assignee: task.assignee,
//             service_number: task.service_number,
//             due_date: task.due_date,
//             priority: task.priority,
//             completed: task.completed,
//             status: task.status,
//             created_at: task.created_at
//         }
//     }
// }

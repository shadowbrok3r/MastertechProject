use crate::{schema::{Priority, Record, Store, User, TASK_TABLE}, DATABASE};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use structdiff::{Difference, StructDiff};
use surrealdb::{sql::Datetime, RecordId};
use chrono::{DateTime, Utc};
use std::cmp::Reverse;

use super::{ComputerData, CustomerData, Status, TaskNotePayload, TicketData, TicketPayload, USER_TABLE};

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
}

pub trait FilterLiveTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<LiveTaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<LiveTaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<LiveTaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<LiveTaskPayload>;
    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<LiveTaskPayload>;
    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<LiveTaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug> (
        &self,
        name: T,
        search_input: String,
    ) -> Vec<LiveTaskPayload>;
}

impl FilterLiveTasks for Vec<LiveTaskPayload> {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.assignee == assignee.get_id())
            .cloned()
            .collect()
    }

    fn filter_by_completion(&self, completed: bool) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.completed == completed)
            .cloned()
            .collect()
    }

    fn filter_by_status(&self, status: &Status) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.status == *status)
            .cloned()
            .collect()
    }

    fn filter_by_priority(&self, priority: &Priority) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.priority == *priority)
            .cloned()
            .collect()
    }

    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.due_date >= date.into())
            .cloned()
            .collect()
    }

    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| {
                assignee.get_store() == *store && task.assignee.key().to_string() == assignee.get_id().key().to_string()
            })
            .cloned()
            .collect()
    }

    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug> (
        &self,
        search: T,
        search_input: String,
    ) -> Vec<LiveTaskPayload> {
        // Create a fuzzy matcher with default settings, ignoring case
        let matcher = SkimMatcherV2::default().ignore_case();

        // If search input is empty, return no tasks
        if search_input.trim().is_empty() {
            return vec![];
        }

        // Pre-filter search to reduce fuzzy match calls
        let search_input_lower = search_input.to_lowercase();
        let match_results = search
            .into_iter()
            .filter(|s| s.as_ref().to_lowercase().contains(&search_input_lower))
            .filter_map(|s| {
                let s_str = s.as_ref();
                // Use fuzzy matching, with fallback for single-letter inputs
                matcher.fuzzy_indices(s_str, &search_input).map(|(score, indices)| {
                    let adjusted_score = if search_input.len() == 1 {
                        score.max(1) // Ensure single-letter matches have a positive score
                    } else {
                        score
                    };
                    (s, adjusted_score, indices)
                })
            })
            .collect::<Vec<_>>();

        // Create a map of task IDs to tasks for O(1) lookups
        let task_map: std::collections::HashMap<_, _> = self
            .iter()
            .map(|task| (task.id.key().to_string(), task))
            .collect();

        // Collect tasks with their best match scores
        let mut task_scores = vec![];
        let mut seen_ids = std::collections::HashSet::with_capacity(self.len());

        for (output, input_score, _) in match_results {
            let output_str = output.as_ref();
            for task in self.iter() {
                let task_id = task.id.key().to_string().clone();
                if seen_ids.contains(&task_id) {
                    continue;
                }
                let task_name = task.task_name.as_str();
                // Compute fuzzy match score for task_name only
                if let Some((task_score, _)) = matcher.fuzzy_indices(task_name, output_str) {
                    let combined_score = task_score.max(input_score);
                    task_scores.push((task_id.clone(), combined_score));
                    seen_ids.insert(task_id);
                }
            }
        }

        // Sort tasks by score in descending order
        task_scores.sort_by_key(|&(_, score)| Reverse(score));

        // Collect unique tasks, avoiding clones
        task_scores
            .into_iter()
            .filter_map(|(task_id, _)| task_map.get(&task_id).map(|task| *task))
            .cloned()
            .collect::<Vec<LiveTaskPayload>>()
    }
}

#[derive(Default, PartialEq, Clone, serde::Serialize, Debug, serde::Deserialize)]
pub enum SortDirection{
    #[default]
    Asc,
    Desc
}

pub trait Sortable <T> {
    fn default_sort(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
}


impl Sortable<LiveTaskPayload> for Vec<LiveTaskPayload> {
    fn default_sort(&mut self,  sort_direction: SortDirection) -> &mut Vec<LiveTaskPayload> {
        let priority_mapping = |priority: &Priority| -> i32 {
            match priority {
                Priority::Express => 2,
                Priority::Rfs => 3,
                Priority::Fire => 4,
                Priority::Qc => 1,
                Priority::Normal => 0,
            }
        };

        self.sort_by(|a, b| {
            let date_a: DateTime<Utc> = a.due_date.clone().into();
            let date_b: DateTime<Utc> = b.due_date.clone().into();

            if date_a < date_b {
                match sort_direction {
                    SortDirection::Asc => return std::cmp::Ordering::Less,
                    SortDirection::Desc => return std::cmp::Ordering::Less.reverse(),
                }
            } else if date_a > date_b {
                match sort_direction {
                    SortDirection::Asc => return std::cmp::Ordering::Greater,
                    SortDirection::Desc => return std::cmp::Ordering::Greater.reverse(),
                }
            } else {
                let priority_a = priority_mapping(&a.priority);
                let priority_b = priority_mapping(&b.priority);
                let ordering = priority_b.cmp(&priority_a);
                match sort_direction {
                    SortDirection::Asc => return ordering,
                    SortDirection::Desc => return ordering.reverse(),
                }
            }
        });

        self
    }
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<LiveTaskPayload>{
        self.sort_by(|a: &LiveTaskPayload, b: &LiveTaskPayload| {
            let date_a: DateTime<Utc> = a.due_date.clone().into();
            let date_b: DateTime<Utc> = b.due_date.clone().into();
            
            let ordering = date_a.cmp(&date_b);
            
            match sort_direction {
                SortDirection::Asc => ordering,               // Use default ordering for ascending
                SortDirection::Desc => ordering.reverse(),    // Reverse ordering for descending
            }
        });
    
        self
    }
    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<LiveTaskPayload> {
        self.sort_by(|a, b| {
            let name_a = &a.task_name.to_lowercase();
            let name_b = &b.task_name.to_lowercase();
            
            let ordering = name_a.cmp(name_b);
    
            match sort_direction {
                SortDirection::Asc => ordering,              // Default alphabetical ordering (A-Z)
                SortDirection::Desc => ordering.reverse(),   // Reverse alphabetical ordering (Z-A)
            }
        });
    
        self
    }
}

impl LiveTaskPayload {
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
}
use crate::{schema::{helper_traits::{parse_email_user, EmployeeHelper}, prestashop_schema::{CustomerMessage, CustomerThread}, Notification, Record, TASK_NOTE_TABLE}, DATABASE};
use structdiff::{Difference, StructDiff};
use surrealdb::{sql::Datetime, RecordId};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use regex::Regex;

use super::{helper_traits::PrestaResourceResponse, prestashop_schema::{self, Employee, Prestashop}, User};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TaskNotePayload {
    pub id: RecordId,
    pub task_id: Option<RecordId>,
    // pub everest_initials: String,
    pub created_at: Datetime,
    pub note: String,
    pub username: String,
    pub id_customer_thread: Option<String>,
    pub id_customer_message: Option<String>,
    pub id_employee: Option<String>,
    pub user: Option<RecordId>,
    // #[serde(deserialize_with = "deserialize_to_string")]
    pub service_number: Option<String>,
    pub private: bool,
}

impl Default for TaskNotePayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TASK_NOTE_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            task_id: Default::default(),
            // everest_initials: Default::default(),
            created_at: Utc::now().into(),
            note: Default::default(),
            username: Default::default(),
            id_customer_thread: Default::default(),
            id_customer_message: Default::default(),
            id_employee: Default::default(),
            user: Default::default(),
            service_number: Default::default(),
            private: false,
        }
    }
}

impl TaskNotePayload {
    /// Creates a task note record in the system.
    ///
    /// # Returns
    /// - `Ok(())` if the creation is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    pub async fn handle_note_creation(&mut self) -> Result<(), anyhow::Error> {
        // If we are creating a prestashop note, then we NEED a service_number, & id_customer_thread, and also assure the
        // note is NOT private.
        let is_prestashop_note = self.id_customer_thread.is_some() && self.service_number.is_some();

        if self.id_employee.is_none() {
            return Err(anyhow::anyhow!("We need an employee ID to create notes"));
        }

        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            self.get_thread_id_from_order().await?
        };

        if is_prestashop_note && !self.private
        {
            self.id_customer_thread = Some(id_customer_thread);
            // Is this sent from the website or mastertech?
            log::info!("task_note/mod.rs -> Sent from website, {:?} - {:?}", self.id_customer_thread, self.id_employee);
            let response = self.create_customer_message().await?;
            log::info!("task_note/mod.rs -> handle_note_creation -> Before struct diffing TaskNotePayload: {:?}", self.clone());
            // Update task note with Prestashop details
            let id = if self.id.key().to_string().is_empty() {
                let task_note_default = TaskNotePayload::default();
                log::info!("task_note/mod.rs -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
                task_note_default.id
            } else {
                if !response.id.to_string().is_empty() {
                    let id = RecordId::from((TASK_NOTE_TABLE, response.id.to_string().clone()));
                    log::info!("task_note/mod.rs -> handle_note_creation -> id is not empty, creating with cust message id: {id:?}");
                    id
                } else {
                    let task_note_default = TaskNotePayload::default();
                    log::info!("task_note/mod.rs -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
                    task_note_default.id
                }
            };

            let updated_value = TaskNotePayload {
                id,
                id_customer_message: Some(response.id.to_string().clone()),
                id_customer_thread: self.id_customer_thread.clone(),
                ..self.clone() // Keep other fields the same
            };

            let diffs = self.diff(&updated_value);
            self.apply_mut(diffs);

            log::info!("task_note/mod.rs -> handle_note_creation -> After struct diffing TaskNotePayload: {:?}", self.clone());

            self.create_task_note_in_db().await?;

        } else if id_customer_thread.is_empty() && self.service_number.is_some() {
            let create_thread_response = self.create_customer_thread().await?;
            log::info!("task_note/mod.rs -> handle_note_creation -> We do NOT have a customer thread ID, and we HAVE a service number, creating thread.");
            self.id_customer_thread = Some(create_thread_response.id.clone());
            let utc_dt: Datetime = DateTime::parse_from_rfc3339(&create_thread_response.date_add)?.with_timezone(&Utc).into();   
            self.created_at = utc_dt;
            if self.id_customer_message.is_none()
                && !create_thread_response.id.is_empty()
                && self.id_employee.is_some()
            {
                // Is this sent from the website or mastertech?
                log::info!("task_note/mod.rs -> Sent from website, {:?} - {:?}", self.id_customer_thread, self.id_employee);
                let response = self.create_customer_message().await?;
                log::info!("task_note/mod.rs -> handle_note_creation -> Before struct diffing TaskNotePayload: {:?}", self.clone());
                // Update task note with Prestashop details
                let id = if self.id.key().to_string().is_empty() {
                    let task_note_default = TaskNotePayload::default();
                    log::info!("task_note/mod.rs -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
                    task_note_default.id
                } else {
                    if !response.id.to_string().is_empty() {
                        let id = RecordId::from((TASK_NOTE_TABLE, response.id.to_string().clone()));
                        log::info!("task_note/mod.rs -> handle_note_creation -> id is not empty, creating with cust message id: {id:?}");
                        id
                    } else {
                        let task_note_default = TaskNotePayload::default();
                        log::info!("task_note/mod.rs -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
                        task_note_default.id
                    }
                };

                let updated_value = TaskNotePayload {
                    id,
                    id_customer_message: Some(response.id.to_string().clone()),
                    id_customer_thread: self.id_customer_thread.clone(),
                    ..self.clone() // Keep other fields the same
                };

                let diffs = self.diff(&updated_value);
                self.apply_mut(diffs);

                log::info!("task_note/mod.rs -> handle_note_creation -> After struct diffing TaskNotePayload: {:?}", self.clone());

                self.create_task_note_in_db().await?;
            }
        } else if id_customer_thread.is_empty() && self.service_number.is_none() {
            log::info!("task_note/mod.rs -> handle_note_creation -> We do NOT have a customer thread ID, and we do NOT have a service number. creating a regular task note. {:?}", self.clone());
            if self.task_id.is_none() {
                return Err(anyhow::anyhow!("Task ID is empty"));
            }
            self.create_task_note_in_db().await?

        } else { 
            // let user = get_current_user_from_auth().await?;
            // if let Some(usr) = user {
            //     if self.username.is_empty() {
            //         self.username = parse_email_user(&usr.email).to_string();
            //     }
    
            //     if self.everest_initials.is_empty() {
            //         self.username = usr.everest_initials;
            //     }

            //     if self.id_employee.is_none() {
            //         self.id_employee = Some(format!("{}", usr.id_prestashop.unwrap_or(0)));
            //     }
            // }

            // The case that this happens, probably need to upsert the task note
            // to match a new service order number than the existing note has or something.
            log::info!("task_note/mod.rs -> handle_note_creation -> This is an odd case... {:?}", self.clone());
            let upsert_note_record: Option<TaskNotePayload> = DATABASE
                .upsert(self.id.clone())
                .content(self.clone())
                .await?;

            log::info!("upsert_note_record: {upsert_note_record:?}");
        }
        self.check_tagged_user_in_note().await?;

        Ok(())
    }

    /// Checks if a user is tagged in a note and updates the note if necessary.
    ///
    /// # Returns
    /// - `Ok(())` if the check is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the check or update.
    pub async fn check_tagged_user_in_note(&mut self) -> anyhow::Result<(), anyhow::Error> {
        let re = Regex::new(r"@\b[a-zA-Z]+(\.[a-zA-Z]+)?\b")?;
        let note = self.note.clone();
        let users: Vec<&str> = re.find_iter(&note).map(|m| m.as_str()).collect();
        let task_id = self.task_id.clone();
        for user_tag in users {
            // Remove '@' from the tag to get the user's name
            let name = &user_tag[1..];
            let email = format!("{}@pclaptops.com", name);
            let mut employee = Employee::default();
            employee.email = email;
            // Simulate database query for user with the email
            let tagged_user: Option<User> = employee.find_user().await?;
            if let (Some(id), Some(tagged_user)) = (task_id.clone(), tagged_user) {
                log::info!("task_note/mod.rs -> check_tagged_user_in_note -> There is an ID, and there IS a tagged user: {id:?} / {tagged_user:?}");
                let task_name: Option<String> = DATABASE
                    .query("SELECT VALUE task_name FROM task WHERE id == $task_id")
                    .bind(("task_id", task_id.clone()))
                    .await?
                    .take(0)?;

                log::info!("task_note/mod.rs -> check_tagged_user_in_note -> Task Name: {:?}", task_name);
                let name = if let Some(name) = task_name {
                    name
                } else {
                    id.to_string()
                };
                // Create notification
                let notification = Notification {
                    notification_description: format!(
                        "tagged {} in task {}",
                        tagged_user.get_username(),
                        name
                    ),
                    notification_type: String::from("Task Update"),
                    status: String::from("Unread"),
                    user: tagged_user.get_id(),
                    ..Default::default()
                };
                self.create_tagged_user_notification(notification).await?;
                self.update_task_note_with_tagged_user(tagged_user.get_id())
                    .await?;
            } // else if let Some(id) = task_id.clone() {
        }

        Ok(())
    }
    
    /// Check to see if a Customer Message already exists
    /// in SurrealDB, to ensure we are not causing weirdness
    /// with the bridge from SurrealDB <-> Prestashop
    /// when deleting, editing, creating notes, etc, as well
    /// as ensuring we always have the synced notes from
    /// prestashop, this will ideally be called as well
    /// when user clicks a "sync with prestashop" or
    /// something
    ///
    /// # Returns
    /// - `Ok(Option<RecordId>)` ID of the record that already exists
    /// in the database with the given criteria, or None if we need to create
    /// a task_note with that message id
    /// - `Err(Error)` if an error occurs during checks
    /// / queries in SurrealDB to find existing notes
    pub async fn check_existing_note_record(&mut self, msg_id: &String) -> anyhow::Result<Option<RecordId>, anyhow::Error> {
        let query_results: Vec<TaskNotePayload> = DATABASE
            .query(
                r#"
                SELECT * FROM task_note 
                    WHERE id_customer_message == $id_customer_message
            "#,
            )
            .bind(("id_customer_message", msg_id.clone()))
            .await?
            .take(0)?;


        log::info!("task_note/mod.rs -> Message existence query results: {query_results:?}");
        
        for note in query_results.iter() {
            log::warn!("task_note/mod.rs -> existing note Task ID: {:?}", note.task_id);
            log::warn!("task_note/mod.rs -> self.note Task ID: {:?}", self.task_id);
            if let (Some(existing_task_id), Some(task_id)) = (&note.task_id, &self.task_id) {
                if existing_task_id != task_id {
                    log::info!("task_note/mod.rs -> UPDATE task_note SET task_id = {task_id:?} WHERE id == {:?}", note.id);
                    let res: Option<TaskNotePayload> = DATABASE
                        .query("UPDATE task_note SET task_id = $new_id WHERE id == $id")
                        .bind(("id", note.id.clone()))
                        .bind(("new_id", task_id.clone()))
                        .await?
                        .take(0)?;

                    log::info!("Task note already exists: {res:#?}, but it should now have a new task id: {task_id:?}");
                }
            }

            log::warn!("task_note/mod.rs -> note.id != self.id: {:?} {:?}", note.id, self.id);
            
            if note.id != self.id {
                return Ok(Some(note.id.clone()));
            }
        }
        Ok(None)
    }

    /// Creates a notification record based on the task note changes.
    ///
    /// # Parameters
    /// - `notification`: The notification payload to create.
    ///
    /// # Returns
    /// - `Ok(())` if the creation is successful.
    /// - `Err(anyhow::Error)` if an error occurs during creation.
    pub async fn create_tagged_user_notification(&mut self, notification: Notification) -> anyhow::Result<(), anyhow::Error> {
        log::info!("task_note/mod.rs -> Creating notification: {:?}", notification);
        let _: Option<Record> = DATABASE
            .query("CREATE notification CONTENT $notif")
            .bind(("notif", notification))
            .await?
            .take(0)?;

        Ok(())
    }

    /// Creates the task note record in the database.
    ///
    /// # Returns
    /// - `Ok(())` if the task_note created successfully.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    pub async fn create_task_note_in_db(&mut self) -> anyhow::Result<(), anyhow::Error> {
        let _: Option<Record> = DATABASE
            .query("CREATE task_note CONTENT $task_note")
            .bind(("task_note", self.clone()))
            .await?
            .take(0)?;
        Ok(())
    }

    /// Creates a customer thread in Prestashop so we can create messages for it.
    ///
    /// # Returns
    /// - `Ok(PrestaResourceResponse)` on successful creation.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    pub async fn create_customer_thread(&mut self) -> anyhow::Result<PrestaResourceResponse, anyhow::Error> {
        if self.service_number.is_none() {
            return Err(anyhow::anyhow!("service number is empty")).into();
        };
        
        let presta_api = Prestashop::default();

        let order: prestashop_schema::Order = presta_api
            .request_subresources_by_id_wasm("orders", "order", &self.service_number.clone().unwrap_or_default())
            .await?;

        Ok(
            presta_api.create_customer_thread(
                &self.service_number.clone().unwrap_or_default(), 
                &order.id_customer
            ).await?
        )
    }

    /// Creates a task note in the Prestashop system.
    ///
    /// # Returns
    /// - `Ok(PrestaResourceResponse)` on successful creation.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    pub async fn create_customer_message(&mut self) -> anyhow::Result<PrestaResourceResponse, anyhow::Error> {
        if self.id_employee.is_none() {
            return Err(anyhow::anyhow!("create_customer_message -> We need a employee ID"))
        }

        let thread_id = self.get_thread_id_from_order().await?;
        let id_employee = self.id_employee.as_deref();

        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            thread_id
        };

        // Check if id_employee or id_customer_thread is empty
        if id_employee.is_empty() || id_customer_thread.is_empty(){
            return Err(anyhow::anyhow!("id_employee or id_customer_thread is empty: {}\n{}", id_employee, id_customer_thread)).into();
        }

        let presta_api = Prestashop::default();
        Ok(
            presta_api.create_customer_message(
                &id_employee, 
                &id_customer_thread,
                &self.note
            ).await?
        )
    }

    /// Updates a task note with information about a tagged user.
    ///
    /// # Parameters
    /// - `user_id`: The ID of the tagged user.
    ///
    /// # Returns
    /// - `Ok(())` if the update is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the update.
    pub async fn update_task_note_with_tagged_user(&mut self, user_id: RecordId) -> anyhow::Result<(), anyhow::Error> {
        log::info!(
            "Updating {:?} with tagged_user: {:?}",
            self.id.clone(),
            user_id
        );
        let _: Option<Record> = DATABASE
            .query("UPDATE task_note SET tagged_users += $user_id WHERE id == $id")
            .bind(("user_id", user_id))
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;
        Ok(())
    }

    /// Modified a task_note and updates the corresponding
    /// note in prestashop
    ///
    /// # Returns
    /// - `Ok(())` if the modification is successful.
    /// - `Err(Error)` if an error occurs during modification.
    pub async fn update_note(&mut self) -> anyhow::Result<(), anyhow::Error> {
        let id_customer_message = self.id_customer_message.clone();
        let id_employee = self.id_employee.clone();
        match self.id_customer_thread.as_ref()  {
            Some(id_customer_thread) => {
                // PRESTASHOP NOTE
                if id_customer_message.is_some()  && id_employee.is_some() {
                    let id_customer_message = id_customer_message.unwrap_or_default();
                    let id_employee = id_employee.unwrap_or_default();
                    let presta = Prestashop::default()
                        .modify_customer_message(
                            &id_customer_message, 
                            &id_employee,
                            &id_customer_thread,
                            &self.note
                        )
                        .await?;
                    log::info!("PrestaResource: {presta:?}");
                } else {
                    return Err(anyhow::anyhow!(
                        "One of (id_customer_message, id_customer_thread, id_employee) is empty\n{:#?}", self.clone()
                    ));
                }
            },
            None => {
                        // REGULAR TASK NOTE / NOT A PRESTASHOP NOTE
                if id_customer_message.is_none() {
                    let upsert_note_record: Option<TaskNotePayload> = DATABASE
                        .upsert(self.id.clone())
                        .content(self.clone())
                        .await?;

                    log::info!("upsert_note_record: {upsert_note_record:?}");
                }
            },
        }
        Ok(())
    }

    /// Deletes a note from either Prestashop or DB or both
    ///
    /// # Returns
    /// - `Ok(())` if the deletion is successful.
    /// - `Err(Error)` if an error occurs during deletion.
    pub async fn delete_note(&mut self) -> anyhow::Result<(), anyhow::Error> {
        let id = self.id.clone();
        log::info!("task_note/mod.rs -> deleting id: {:?}", &id);
        if let (Some(thread_id), Some(message_id)) = (
            self.id_customer_thread.as_ref(),
            self.id_customer_message.as_ref(),
        ) {
            if !thread_id.is_empty() && !message_id.is_empty() {
                self.delete_prestashop_note().await?;
            } else {
                log::info!("task_note/mod.rs -> Thread ID or Message ID is empty: {thread_id:?} / {message_id:?}");
                let delete_res = self.delete_task_note().await;
                log::info!("task_note/mod.rs -> delete_res: {delete_res:?}");
            }
        } else {
            log::info!("task_note/mod.rs -> Not deleting prestashop note, there is either no thread id or no message id");
            let delete_res = self.delete_task_note().await;
            log::info!("task_note/mod.rs -> Deleted note: {:?}", delete_res);
        }
        Ok(())
    }

    pub async fn delete_task_note(&mut self) -> anyhow::Result<(), anyhow::Error> {
        let id = self.id.clone();
        log::info!("schema/task_note/mod.rs -> deleting id: {:?}", id.clone());
        let _: Option<Record> = DATABASE
            .delete((TASK_NOTE_TABLE, id.key().to_string()))
            .await?;
        Ok(())
    }

    /// Deletes a note from prestashop. This will only
    /// happen if there IS an id_customer_message as
    /// well as an id_customer_thread.
    ///
    /// # Returns
    /// - `Ok(())` if the deletion is successful.
    /// - `Err(Error)` if an error occurs during deletion.
    pub async fn delete_prestashop_note(&mut self) -> anyhow::Result<(), anyhow::Error> {
        if let Some(cust_msg_id) = self.id_customer_message.as_ref() {
            if !cust_msg_id.is_empty() {
                let prestashop = Prestashop::default();
                let delete_result = prestashop
                    .delete_resource_wasm("customer_messages", &cust_msg_id.clone())
                    .await?;

                log::info!("task_note/mod.rs -> Delete Result for customer message: {delete_result:?}");

                let delete_res: Option<Record> = DATABASE
                    .query("DELETE task_note WHERE id_customer_message == $id_customer_message")
                    .bind(("id_customer_message", cust_msg_id.clone()))
                    .await?
                    .take(0)?;

                log::info!("task_note/mod.rs -> Deleted note: {:?}", delete_res);
            }
        }
        Ok(())
    }

    /// Retrieves the service number associated with a task using task ID
    ///
    /// # Returns
    /// - `Ok(String)` containing the order ID on success.
    /// - `Err(Error)` if the order cannot be found or an error occurs.
    pub async fn get_order_by_task_id(&mut self) -> anyhow::Result<String> {
        if let Some(id) = self.task_id.clone() {
            let service_number: Option<String> = DATABASE
                .query("SELECT VALUE service_number FROM task WHERE id == $task_id")
                .bind(("task_id", id.clone()))
                .await?
                .take(0)?;
            log::info!("service_number ({service_number:?}) pulled from task.id using: {id:?}");
            Ok(service_number.unwrap_or_default())
        } else {
            Err(anyhow::anyhow!("task_note/mod.rs -> get_order_by_task_id -> No order number found"))
        }
    }

    /// Retrieves the thread ID based on an order.
    ///
    /// # Returns
    /// - `Ok(String)` containing the thread ID on success.
    /// - `Err(Error)` if the thread ID cannot be found or an error occurs.
    pub async fn get_thread_id_from_order(&mut self) -> anyhow::Result<String> {
        let mut threads = Vec::new();
        match self.get_order_by_task_id().await {
            Ok(service_number) => {
                log::info!("task_note/mod.rs -> Calling API for thread ID");
            
                if service_number.is_empty() {
                    log::info!("task_note/mod.rs -> Service number is empty. not querying Presta {service_number:?}");
                    return Err(anyhow::anyhow!("service number is empty")).into();
                }
                
                let api_call = Prestashop::default();
                let mut query: HashMap<&str, &str> = HashMap::new();
    
                query.insert("filter[id_order]", &service_number);
                query.insert("output_format", "JSON");

                let customer_threads: Vec<CustomerThread> = api_call
                    .request_resources_wasm("customer_threads", query.clone())
                    .await?;
                
                log::info!("task_note/mod.rs -> get_thread_id_from_order -> Got customer threads: {customer_threads:?}");
                if customer_threads.is_empty() && self.id_customer_thread.is_none() && self.service_number.is_some() {
                    let create_thread_response = self.create_customer_thread().await?;
                    log::info!("task_note/mod.rs -> handle_note_creation -> We do NOT have a customer thread ID, and we HAVE a service number, creating thread.");
                    self.id_customer_thread = Some(create_thread_response.id);
                    let utc_dt: Datetime = DateTime::parse_from_rfc3339(&create_thread_response.date_add)?.with_timezone(&Utc).into();   
                    self.created_at = utc_dt;
                }

                for thread in customer_threads {
                    threads.push(thread);
                    for msg in thread.associations.customer_messages.iter() {
                        log::info!("task_note/mod.rs -> get_thread_id_from_order -> Checking if msg ID exists in database: {:?}", &msg);
                        let exists = self.check_existing_note_record(&msg.id).await?;
                        if exists.is_none() {
                            log::info!("task_note/mod.rs -> get_thread_id_from_order -> WE NEED TO CREATE THIS MESSAGE IN OUR DATABASE: {msg:?}");
                            let customer_message: CustomerMessage = api_call
                                .request_subresources_by_id_wasm(
                                    "customer_messages",
                                    "customer_message",
                                    &msg.id,
                                )
                                .await?;

                            if !customer_message.id_employee.is_empty() && customer_message.id_employee.as_str() != "0" {
                                let mut employee: Employee = api_call
                                    .request_subresources_by_id_wasm(
                                        "employees",
                                        "employee",
                                        &customer_message.id_employee,
                                    )
                                    .await?; // Employee::default().get_employee_from_id(&customer_message.id_employee).await?;

                                let mut task_note = TaskNotePayload {
                                    id: RecordId::from((TASK_NOTE_TABLE, customer_message.id.clone())),
                                    id_customer_message: Some(customer_message.id.clone()),
                                    id_customer_thread: Some(thread.id.clone()),
                                    task_id: self.task_id.clone(),
                                    id_employee: Some(customer_message.id_employee),
                                    created_at: DateTime::parse_from_rfc3339(&customer_message.date_add)?.with_timezone(&Utc).into(),
                                    note: customer_message.message,
                                    username: parse_email_user(&employee.email).to_string(),
                                    user: if let Some(usr) = employee.find_user().await? { Some(usr.get_id()) } else { None },
                                    service_number: Some(service_number.to_string()),
                                    private: false
                                };

                                log::info!("task_note/mod.rs -> get_thread_id_from_order -> Creating a new task_note: {task_note:?}");

                                match task_note.create_task_note_in_db().await {
                                    Ok(_) => log::info!("Created task note: {task_note:?}"),
                                    Err(e) => log::info!("Error creating task note: {e:?}"),
                                }
                            } else {
                                log::info!("task_note/mod.rs -> get_thread_id_from_order -> Customer Message contains some bad data: {customer_message:#?}");
                            }
                        } else {
                            log::info!("task_note/mod.rs -> get_thread_id_from_order -> Message already exists: {:?}", exists);
                        }
                    }
                }
            }
            Err(e) => { return Err(anyhow::anyhow!("task_note/mod.rs -> {e:?}")).into(); }
        }

        // Extract the first customer thread ID if available
        let id_customer_thread = threads
            .iter()
            .filter_map(|m| Some(m.id.clone()))
            .next()
            .unwrap_or_default();

        log::info!("task_note/mod.rs -> Customer thread id from order: {id_customer_thread:?}");

        Ok(id_customer_thread)
    }

    /// Retrieves the thread ID based on an order NUMBER.
    ///
    /// # Returns
    /// - `Ok(Vec<TaskNotePayload>)` containing the notes from an order.
    /// - `Err(Error)` if the thread ID cannot be found or an error occurs.
    pub async fn get_notes_from_service_number(&mut self, service_number: &str) -> anyhow::Result<Vec<TaskNotePayload>> {
        let mut notes = Vec::new();
        log::info!("task_note/mod.rs -> get_notes_from_service_number -> Calling get_notes_from_service_number");
        
        if !service_number.is_empty() {
            let api_call = Prestashop::default();
            let mut query: HashMap<&str, &str> = HashMap::new();

            query.insert("filter[id_order]", &service_number);
            query.insert("output_format", "JSON");

            let customer_threads: Vec<CustomerThread> = api_call
                .request_resources_wasm("customer_threads", query.clone())
                .await?;
            
            log::info!("task_note/mod.rs -> get_notes_from_service_number -> Got customer threads: {customer_threads:?}");
            for thread in customer_threads {
                for msg in thread.associations.customer_messages.iter() {
                    log::info!("task_note/mod.rs -> get_notes_from_service_number -> Checking if msg ID exists in database: {:?}", &msg);
                    let exists = self.check_existing_note_record(&msg.id).await?;
                    if exists.is_none() {
                        log::info!("task_note/mod.rs -> get_notes_from_service_number -> WE NEED TO CREATE THIS MESSAGE IN OUR DATABASE: {msg:?}");
                        let customer_message: CustomerMessage = api_call
                            .request_subresources_by_id_wasm(
                                "customer_messages",
                                "customer_message",
                                &msg.id,
                            )
                            .await?;

                        let mut employee: Employee = api_call
                            .request_subresources_by_id_wasm(
                                "employees",
                                "employee",
                                &customer_message.id_employee,
                            )
                            .await?; // Employee::default().get_employee_from_id(&customer_message.id_employee).await?;

                        let task_note = TaskNotePayload {
                            id: RecordId::from((TASK_NOTE_TABLE, customer_message.id.clone())),
                            id_customer_message: Some(customer_message.id.clone()),
                            id_customer_thread: Some(thread.id.clone()),
                            task_id: self.task_id.clone(),
                            id_employee: Some(customer_message.id_employee),
                            created_at: DateTime::parse_from_rfc3339(&customer_message.date_add)?.with_timezone(&Utc).into(),
                            note: customer_message.message,
                            username: parse_email_user(&employee.email).to_string(),
                            service_number: Some(service_number.to_string()),
                            user: if let Some(usr) = employee.find_user().await? { Some(usr.get_id()) } else { None },
                            private: false,
                        };
                        log::warn!("task_note/mod.rs -> get_notes_from_service_number -> Creating a new task_note: {task_note:?}");

                        let note: Option<Record> = DATABASE
                            .query("CREATE task_note CONTENT $task_note")
                            .bind(("task_note", task_note.clone()))
                            .await?
                            .take(0)?;

                        notes.push(task_note);

                        log::warn!("task_note/mod.rs -> get_notes_from_service_number -> Created note: {note:?}");
                    } else {
                        log::warn!("task_note/mod.rs -> get_notes_from_service_number -> Message already exists: {:?}", exists);
                        let query_results: Vec<TaskNotePayload> = DATABASE
                            .query("SELECT * FROM task_note WHERE id_customer_message == $id_customer_message")
                            .bind(("id_customer_message", msg.id.clone()))
                            .await?
                            .take(0)?;

                        for note in query_results.iter() {
                            notes.push(note.clone());
                        }
                    }
                }
            }
        } else {
            log::info!("task_note/mod.rs -> get_notes_from_service_number -> Order number is still empty? {service_number:?}");
        }

        Ok(notes)
    }

    pub fn set_task_id(&mut self, task_id: RecordId) -> &mut Self {
        self.task_id = Some(task_id);
        self
    }

    pub async fn get_db_notes_from_service(service_number: String) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        log::debug!("get_task_from_service_number");
        if service_number.is_empty() {
            return Err(anyhow::anyhow!("utilities.rs -> get_task_from_service_number -> Service Number is empty"));
        }
        
        let query_results: Vec<Self> = DATABASE
            .query("SELECT * FROM task_note WHERE task_id.service_number == $service_number PARALLEL")
            .bind(("service_number", service_number.clone()))
            .await?
            .take(0)?;

        if query_results.is_empty() {
            let alt_query: Vec<Self> = DATABASE
                .query("SELECT * FROM task_note WHERE service_number == $service_number PARALLEL")
                .bind(("service_number", service_number))
                .await?
                .take(0)?;
            log::info!("schema/utilities.rs -> get_task_notes_from_service_number: {alt_query:?}");
            Ok(alt_query)
        } else {
            log::info!("schema/utilities.rs -> get_task_notes_from_service_number: {query_results:?}");
            Ok(query_results)
        }
    }
}


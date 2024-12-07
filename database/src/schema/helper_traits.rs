#![allow(async_fn_in_trait)]
use super::{
    prestashop_schema::{self, CustomerMessage, CustomerThread, Employee, Prestashop},
    ComputerData, ConnectedClient, CustomerData, ExtendedSeb, Notification, Record,
    SpecialPartOrder, Store, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User,
    TASK_NOTE_TABLE,
};
use crate::DATABASE;
use anyhow::{Error, Result};
use async_trait::async_trait;
use chrono::{NaiveDateTime, TimeZone, Utc};
use log::{debug, info};
use regex::Regex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};
use structdiff::StructDiff;
use surrealdb::RecordId;

/// Macro to implement GetDataFromId for structs with an 'id' field
macro_rules! _get_id {
    ($struct_name:ident) => {
        #[async_trait(?Send)]
        impl GetDataFromId for $struct_name {
            async fn get_id(&mut self) -> &RecordId {
                &mut self.id
            }
        }
    };
}

/// Get the associated data tied to an ID
#[async_trait(?Send)]
pub trait GetAssociatedDataFromId<D> {
    async fn get_associated_data<T>(&mut self) -> Result<D, Error>
    where
        T: Serialize + for<'de> Deserialize<'de> + Clone,
        D: structdiff::StructDiff
            + DeserializeOwned
            + Serialize
            + 'static
            + Debug
            + std::marker::Unpin
            + for<'de> Deserialize<'de>;
}

/// A trait for assisting with operations involving the Employee struct
pub trait EmployeeHelper {
    /// Find a User based on Employee info -> id_employee
    async fn find_user(&mut self) -> Result<Option<User>, Error>;
    /// Pull all of my services given Employee info -> id_employee
    async fn get_my_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all orders in my store given Employee info -> id_location
    async fn get_services_in_my_store(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all Orders of which are my Return For Service's
    async fn get_my_return_for_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all Return For Service's in my store
    async fn get_my_store_return_for_services(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get Employee from ID
    async fn get_employee_from_id(&mut self, id_employee: &str) -> Result<Employee, Error>;
}

/// A trait for assisting with operations involving the `User` struct.
pub trait UserHelper {
    /// Finds and retrieves the associated Employee record based on the User information.
    ///
    /// # Returns
    /// - `Ok(Employee)` on success, where `Employee` is a struct representing the employee record.
    /// - `Err(Error)` if the employee cannot be found or an error occurs during the operation.
    async fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error>;

    /// Saves the user settings to the database or persistent storage.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(Error)` if an error occurs while saving the settings.
    async fn save_user_ui_layout(&mut self) -> Result<(), Error>;

    /// Retrieves the store number from the Odoo system.
    ///
    /// # Returns
    /// - `Ok(u64)` containing the store number on success.
    /// - `Err(Error)` if the store number cannot be retrieved or an error occurs.
    fn get_odoo_store_number(&mut self) -> Result<u64, Error>;

    /// Retrieves the store details associated with a given Odoo ID.
    ///
    /// # Returns
    /// - `Ok(Store)` containing the store information on success.
    /// - `Err(Error)` if the store cannot be found or an error occurs.
    fn get_store_from_odoo_id(&mut self) -> Result<Store, Error>;

    // async fn save_theme_config() -> Result<(), Error>;
}


/// A trait for assisting with operations involving the ComputerData struct
#[async_trait(?Send)]
pub trait ComputerDataHelper {
    /// Associate a ComputerData record to a ServiceOrder
    async fn associate_to_service(&mut self) -> Result<prestashop_schema::ServiceOrder, Error>;
    /// Find TicketData associated with this Computer
    async fn find_associated_tickets(&mut self) -> Result<Vec<TicketData>, Error>;
    /// Find Clients associated with this Computer
    async fn find_associated_client(&mut self) -> Result<ConnectedClient, Error>;
    /// Find Tasks associated to this Computer
    async fn find_associated_tasks(&mut self) -> Result<Vec<TaskPayload>, Error>;
    /// Find Customer that owns this Computer
    async fn find_associated_customer(&mut self) -> Result<CustomerData, Error>;
    /// Find PrestaShop Orders associated with this Computer
    async fn find_associated_prestashop_orders(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Find PrestaShop Customer associated with this Computer
    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
}

/// A trait for assisting with operations involving Customer Records.
#[async_trait(?Send)]
pub trait CustomerHelper {
    /// Finds and retrieves the associated address for a customer.
    ///
    /// # Returns
    /// - `Ok(Address)` containing the customer's address on success.
    /// - `Err(Error)` if the address cannot be found or an error occurs.
    async fn find_associated_addr(&mut self) -> Result<prestashop_schema::Address, Error>;
}

/// A trait for managing and assisting with Customer Data operations.
#[async_trait(?Send)]
pub trait CustomerDataHelper {
    /// Finds and retrieves special part orders for a customer.
    ///
    /// # Returns
    /// - `Ok(Vec<SpecialPartOrder>)` containing a list of special part orders.
    /// - `Err(Error)` if an error occurs during the retrieval of orders.
    async fn find_part_orders(&mut self) -> Result<Vec<SpecialPartOrder>, Error>;

    /// Finds and retrieves a customer record from Prestashop.
    ///
    /// # Returns
    /// - `Ok(Customer)` containing the Prestashop customer record on success.
    /// - `Err(Error)` if an error occurs during retrieval.
    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;

    /// Retrieves and returns extended SEB data for a customer.
    ///
    /// # Returns
    /// - `Ok(ExtendedSeb)` containing the extended SEB data.
    /// - `Err(Error)` if an error occurs during retrieval.
    async fn get_seb_data(&mut self) -> Result<ExtendedSeb, Error>;
}

/// A trait for assisting with operations involving orders.
#[async_trait(?Send)]
pub trait OrderHelper {
    /// Converts an order to a task payload structure.
    ///
    /// # Returns
    /// - `Ok(TaskPayload)` on success.
    /// - `Err(Error)` if an error occurs during conversion.
    async fn convert_to_task_payload(&mut self) -> Result<TaskPayload, Error>;

    /// Converts an order to a ticket payload structure.
    ///
    /// # Returns
    /// - `Ok(TicketPayload)` on success.
    /// - `Err(Error)` if an error occurs during conversion.
    async fn convert_to_ticket_payload(&mut self) -> Result<TicketPayload, Error>;

    /// Retrieves all return orders associated with services.
    ///
    /// # Returns
    /// - `Ok(Vec<Order>)` containing a list of return orders.
    /// - `Err(Error)` if an error occurs during retrieval.
    async fn get_all_return_for_services(&mut self)
        -> Result<Vec<prestashop_schema::Order>, Error>;
}

/// A trait for managing operations and data related to task note payloads.
pub trait TaskNotePayloadHelper: Send {
    /// Creates a task note in the Prestashop system.
    ///
    /// # Returns
    /// - `Ok(Response)` on successful creation.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    async fn create_prestashop_note(&mut self) -> Result<Response, anyhow::Error>
    where
        anyhow::Error: Send;

    /// Checks if a user is tagged in a note and updates the note if necessary.
    ///
    /// # Returns
    /// - `Ok(())` if the check is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the check or update.
    async fn check_tagged_user_in_note(&mut self) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Creates a task note record in the system.
    ///
    /// # Returns
    /// - `Ok(())` if the creation is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    async fn create_task_note(&mut self) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Creates the task note record in the database.
    ///
    /// # Returns
    /// - `Ok(())` if the task_note created successfully.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    async fn create_task_note_in_db(&mut self) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Updates the fields of an existing task note.
    ///
    /// # Returns
    /// - `Ok(())` if the update is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the update.
    async fn update_task_note_fields(&mut self) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Updates the `created_at` field of a task note to the current time.
    ///
    /// # Returns
    /// - `Ok(())` if the update is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the update.
    async fn update_task_note_with_current_time(&mut self) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Updates the username field of a task note if it is missing or incorrect.
    ///
    /// # Returns
    /// - `Ok(())` if the update is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the update.
    async fn update_username_if_needed(&mut self) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Creates a notification record based on the task note changes.
    ///
    /// # Parameters
    /// - `notification`: The notification payload to create.
    ///
    /// # Returns
    /// - `Ok(())` if the creation is successful.
    /// - `Err(anyhow::Error)` if an error occurs during creation.
    async fn create_notification(
        &mut self,
        notification: Notification,
    ) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Updates a task note with information about a tagged user.
    ///
    /// # Parameters
    /// - `user_id`: The ID of the tagged user.
    ///
    /// # Returns
    /// - `Ok(())` if the update is successful.
    /// - `Err(anyhow::Error)` if an error occurs during the update.
    async fn update_task_note_with_tagged_user(
        &mut self,
        user_id: RecordId,
    ) -> Result<(), anyhow::Error>
    where
        anyhow::Error: Send;

    /// Retrieves the thread ID based on an order.
    ///
    /// # Returns
    /// - `Ok(String)` containing the thread ID on success.
    /// - `Err(Error)` if the thread ID cannot be found or an error occurs.
    async fn get_thread_id_from_order(&mut self) -> Result<String>
    where
        anyhow::Error: Send;

    /// Retrieves the order ID associated with a task.
    ///
    /// # Returns
    /// - `Ok(String)` containing the order ID on success.
    /// - `Err(Error)` if the order cannot be found or an error occurs.
    async fn get_order_by_task_id(&mut self) -> Result<String>
    where
        anyhow::Error: Send;

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
    async fn check_existing_note_record(
        &mut self,
        msg_id: &String,
    ) -> Result<Option<RecordId>, Error>
    where
        anyhow::Error: Send;

    /// Modified a task_note and updates the corresponding
    /// note in prestashop
    ///
    /// # Returns
    /// - `Ok(())` if the modification is successful.
    /// - `Err(Error)` if an error occurs during modification.
    async fn modify_prestashop_note(&mut self) -> Result<Response, Error>;

    /// Deletes a note from the system.
    ///
    /// # Returns
    /// - `Ok(())` if the deletion is successful.
    /// - `Err(Error)` if an error occurs during deletion.
    async fn delete_note(&mut self) -> Result<(), Error>
    where
        anyhow::Error: Send;

    /// Deletes a note from prestashop. This will only
    /// happen if there IS an id_customer_message as
    /// well as an id_customer_thread.
    ///
    /// # Returns
    /// - `Ok(())` if the deletion is successful.
    /// - `Err(Error)` if an error occurs during deletion.
    async fn delete_prestashop_note(&mut self) -> Result<(), Error>
    where
        anyhow::Error: Send;
}

impl TaskNotePayloadHelper for TaskNotePayload {
    async fn check_tagged_user_in_note(&mut self) -> Result<(), Error> {
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
                info!(
                    "There is an ID, and there IS a tagged user: {:?} / {:?}",
                    id, tagged_user
                );
                let task_name: Option<String> = DATABASE
                    .query("SELECT VALUE task_name FROM task WHERE id == $task_id")
                    .bind(("task_id", task_id.clone()))
                    .await?
                    .take(0)?;

                info!("Task Name: {:?}", task_name);
                let name = if let Some(name) = task_name {
                    name
                } else {
                    id.to_string()
                };
                // Create notification
                let notification = Notification {
                    notification_description: format!(
                        "tagged {} in task {}",
                        parse_email_user(&tagged_user.email),
                        name
                    ),
                    notification_type: String::from("Task Update"),
                    status: String::from("Unread"),
                    user: tagged_user.id.clone(),
                    ..Default::default()
                };
                self.create_notification(notification).await?;
                self.update_task_note_with_tagged_user(tagged_user.id.clone())
                    .await?;
            } // else if let Some(id) = task_id.clone() {
        }

        Ok(())
    }

    async fn create_notification(&mut self, notification: Notification) -> Result<(), Error> {
        info!("Creating notification: {:?}", notification);
        let _: Option<Record> = DATABASE
            .query("CREATE notification CONTENT $notif")
            .bind(("notif", notification))
            .await?
            .take(0)?;

        Ok(())
    }

    async fn update_task_note_with_tagged_user(&mut self, user_id: RecordId) -> Result<(), Error> {
        info!(
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

    async fn create_task_note(&mut self) -> Result<(), anyhow::Error> {
        if self.created_at.is_empty() {
            self.update_task_note_with_current_time().await?;
        }

        let thread_id = self.get_thread_id_from_order().await?;
        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            thread_id
        };

        if self.id_customer_message.is_none()
            && !id_customer_thread.is_empty()
            && self.id_employee.is_some()
        {
            self.id_customer_thread = Some(id_customer_thread);
            // Is this sent from the website or mastertech?
            info!(
                "Sent from website, {:?} - {:?}",
                self.id_customer_thread, self.id_employee
            );
            let response = self.create_prestashop_note().await?;
            info!("Before struct diffing TaskNotePayload: {:?}", self.clone());
            // Update task note with Prestashop details
            let id = if self.id.key().to_string().is_empty() {
                let task_note_default = TaskNotePayload::default();
                info!(
                    "ID is empty, assigning a new id: {:?}",
                    task_note_default.id
                );
                task_note_default.id
            } else {
                if !response.id.to_string().is_empty() {
                    let id = RecordId::from((TASK_NOTE_TABLE, response.id.to_string().clone()));
                    info!("id is not empty, creating with cust message id: {id:?}");
                    id
                } else {
                    let task_note_default = TaskNotePayload::default();
                    info!(
                        "ID is empty, assigning a new id: {:?}",
                        task_note_default.id
                    );
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

            info!("After struct diffing TaskNotePayload: {:?}", self.clone());

            self.create_task_note_in_db().await?;

            self.update_username_if_needed().await?;
        } else {
            // Handle other cases
            info!("Sent from Mastertech, updating other task note fields");
            // created_at: response.date_add,
            self.update_task_note_fields().await?;
        }
        self.check_tagged_user_in_note().await?;

        Ok(())
    }

    async fn update_username_if_needed(&mut self) -> Result<(), Error> {
        // Logic to update username if needed
        Ok(())
    }

    async fn create_task_note_in_db(&mut self) -> Result<(), Error> {
        let _: Option<Record> = DATABASE
            .query("CREATE task_note CONTENT $task_note")
            .bind(("task_note", self.clone()))
            .await?
            .take(0)?;
        Ok(())
    }

    async fn update_task_note_with_current_time(&mut self) -> Result<(), Error> {
        // Logic to update task note with the current time
        self.created_at = chrono::Utc::now().to_rfc3339();
        info!("Created_at was empty, now it is {:?}", self.created_at);
        Ok(())
    }

    async fn update_task_note_fields(&mut self) -> Result<(), Error> {
        // Logic to update task note fields
        Ok(())
    }

    async fn create_prestashop_note(&mut self) -> Result<Response, Error> {
        let thread_id = self.get_thread_id_from_order().await?;
        let id_employee = self.id_employee.as_deref().unwrap_or("");

        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            thread_id
        };

        // Check if id_employee or id_customer_thread is empty
        if id_employee.is_empty() {
            return Err(anyhow::anyhow!("id_employee is empty")).into();
        }

        if id_customer_thread.is_empty() {
            return Err(anyhow::anyhow!("id_customer_thread is empty")).into();
        }

        // Prepare the XML payload
        let begin = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><prestashop xmlns:xlink=\"http://www.w3.org/1999/xlink\">";
        let end = "</prestashop>";

        let payload = format!(
            "{}<customer_message><id_lang>1</id_lang><id_employee>{}</id_employee><id_customer_thread>{}</id_customer_thread><message>{}</message><private>1</private><id_order_message_type>0</id_order_message_type></customer_message>{}",
            begin, id_employee, id_customer_thread, self.note, end
        );

        // Send HTTP POST request with the XML payload
        let client = reqwest::Client::new();
        info!("Payload: {:?}", payload);
        let response_text = client
            .post("https://pcl.master-tech.app/api/customer_messages")
            .header("Content-type", "application/xml")
            .body(payload)
            .send()
            .await?
            .text()
            .await?;

        info!("response text: {response_text:?}");
        // Parse the XML response to extract values
        let id = response_text
            .split("<id><![CDATA[")
            .nth(1)
            .and_then(|s| s.split("]]></id>").next())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse 'id' from response"))?;

        let date_add = response_text
            .split("<date_add><![CDATA[")
            .nth(1)
            .and_then(|s| s.split("]]></date_add>").next())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse 'date_add' from response"))?;

        let date_upd = response_text
            .split("<date_upd><![CDATA[")
            .nth(1)
            .and_then(|s| s.split("]]></date_upd>").next())
            .unwrap_or(""); // Optional field, so we handle it accordingly

        // Return a Response struct with extracted values
        // Ok(Response {
        //     date_add: String::new(), // convert_date_string(date_add)?.to_string(), //,
        //     id: String::new(), // id.to_string(),
        //     date_upd: String::new(), // convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        // })
        Ok(Response {
            date_add: convert_date_string(date_add)?.to_string(), //,
            id: id.to_string(),
            date_upd: convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        })
    }

    async fn get_order_by_task_id(&mut self) -> Result<String> {
        if let Some(id) = self.task_id.clone() {
            // I cannot do this at the moment because GetAssociatedData requires + Send
            // let task: TaskPayload = id.get_associated_data::<TaskPayload>().await?;
            // let order_number = task.service_number;
            let order_number: Option<String> = DATABASE
                .query("SELECT VALUE service_number FROM task WHERE id == $task_id")
                .bind(("task_id", id.clone()))
                .await?
                .take(0)?;
            info!(
                "Order number pulled from task_id: {order_number:?} using task id: {:?}",
                id
            );
            Ok(order_number.unwrap_or_default())
        } else {
            info!("No order number found");
            Ok(String::new())
        }
    }

    async fn get_thread_id_from_order(&mut self) -> Result<String> {
        let mut threads = Vec::new();
        if let Ok(order_number) = self.get_order_by_task_id().await {
            info!("Calling API for thread ID");
            
            if !order_number.is_empty() {
                let api_call = Prestashop::default();
                let mut query: HashMap<&str, &str> = HashMap::new();
    
                query.insert("filter[id_order]", &order_number);
                query.insert("output_format", "JSON");

                let customer_threads: Vec<CustomerThread> = api_call
                    .request_resources_wasm("customer_threads", query.clone())
                    .await?;
                
                info!("Got customer threads: {customer_threads:?}");
                for thread in customer_threads {
                    for msg in thread.associations.customer_messages.iter() {
                        info!("Checking if msg ID exists in database: {:?}", &msg);
                        let exists = self.check_existing_note_record(&msg.id).await?;
                        if exists.is_none() {
                            info!("WE NEED TO CREATE THIS MESSAGE IN OUR DATABASE: {msg:?}");
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
                                .await?; // Employee::default();
                                         // employee.get_employee_from_id(&customer_message.id_employee).await?;

                            let id = RecordId::from((TASK_NOTE_TABLE, customer_message.id.clone()));
                            let user = if let Some(usr) = employee.find_user().await? {
                                Some(usr.id)
                            } else {
                                None
                            };

                            let task_note = TaskNotePayload {
                                id,
                                id_customer_message: Some(customer_message.id.clone()),
                                id_customer_thread: Some(thread.id.clone()),
                                task_id: self.task_id.clone(),
                                id_employee: Some(customer_message.id_employee),
                                created_at: convert_date_string(&customer_message.date_add)?,
                                note: customer_message.message,
                                username: parse_email_user(&employee.email).to_string(),
                                everest_initials: employee.initials,
                                user,
                            };
                            info!("Creating a new task_note: {task_note:?}");

                            let note: Option<Record> = DATABASE
                                .query("CREATE task_note CONTENT $task_note")
                                .bind(("task_note", task_note))
                                .await?
                                .take(0)?;

                            info!("Created note: {note:?}");
                        } else {
                            info!("Message already exists: {:?}", exists);
                        }
                        info!("Message existence in database: {exists:?}");
                    }
                    threads.push(thread);
                }
            } else {
                info!("Order number is still empty? {order_number:?}");
            }
        } else {
            info!("Error getting order number from task id");
        }

        // Extract the first customer thread ID if available
        let id_customer_thread = threads
            .iter()
            .filter_map(|m| Some(m.id.clone()))
            .next()
            .unwrap_or_default();

        info!("Customer thread id from order: {id_customer_thread:?}");

        Ok(id_customer_thread)
    }

    async fn check_existing_note_record(
        &mut self,
        msg_id: &String,
    ) -> Result<Option<RecordId>, Error> {
        let query_results: Vec<TaskNotePayload> = DATABASE
            .query(
                r#"
                SELECT * FROM task_note 
                    WHERE id_customer_message == $id_customer_message
            "#,
            )
            .bind(("id_customer_message", msg_id.clone()))
            // .bind(("id_customer_thread", thread_id.clone()))
            .await?
            .take(0)?;
        info!("Message existence query results: {query_results:?}");

        for res in query_results.iter() {
            if res.id != self.id {
                return Ok(Some(res.id.clone()));
            }
        }
        Ok(None)
    }

    async fn modify_prestashop_note(&mut self) -> Result<Response, Error> {
        let thread_id = self.get_thread_id_from_order().await?;
        let id_employee = self.id_employee.as_deref().unwrap_or("");

        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            thread_id
        };

        // Check if id_employee or id_customer_thread is empty
        if id_employee.is_empty() {
            return Err(anyhow::anyhow!("id_employee is empty")).into();
        }

        if id_customer_thread.is_empty() {
            return Err(anyhow::anyhow!("id_customer_thread is empty")).into();
        }

        if self.id_customer_message.is_none() {
            return Err(anyhow::anyhow!("id_customer_thread is empty")).into();
        }

        // Prepare the XML payload
        let begin = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><prestashop xmlns:xlink=\"http://www.w3.org/1999/xlink\">";
        let end = "</prestashop>";

        let payload = format!(
            r#"
            {begin}
            <customer_message>
            <id_lang>1</id_lang>
            <id_employee>{id_employee}</id_employee>
            <id_customer_thread>{id_customer_thread}</id_customer_thread>
            <id>{}</id>
            <message>{}</message>
            <private>1</private>
            <id_order_message_type>0</id_order_message_type>
            </customer_message>
            {end}
            "#,
            self.id_customer_message.clone().unwrap(),
            self.note
        );

        // Send HTTP POST request with the XML payload
        let _client = reqwest::Client::new();
        info!("Payload: {:?}", payload);
        // let response_text = client
        //     .post("https://pcl.master-tech.app/api/customer_messages")
        //     .header("Content-type", "application/xml")
        //     .body(payload)
        //     .send()
        //     .await?
        //     .text()
        //     .await?;

        // // Parse the XML response to extract values
        // let id = response_text
        //     .split("<id><![CDATA[")
        //     .nth(1)
        //     .and_then(|s| s.split("]]></id>").next())
        //     .ok_or_else(|| anyhow::anyhow!("Failed to parse 'id' from response"))?;

        // let date_add = response_text
        //     .split("<date_add><![CDATA[")
        //     .nth(1)
        //     .and_then(|s| s.split("]]></date_add>").next())
        //     .ok_or_else(|| anyhow::anyhow!("Failed to parse 'date_add' from response"))?;

        // let date_upd = response_text
        //     .split("<date_upd><![CDATA[")
        //     .nth(1)
        //     .and_then(|s| s.split("]]></date_upd>").next())
        //     .unwrap_or(""); // Optional field, so we handle it accordingly

        // Return a Response struct with extracted values
        Ok(Response {
            date_add: String::new(), //convert_date_string(date_add)?.to_string(), //,
            id: String::new(),       //id.to_string(),
            date_upd: String::new(), //convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        })
    }

    async fn delete_note(&mut self) -> Result<(), Error> {
        let id = self.id.clone();
        info!("deleting id: {:?}", &id);
        if let (Some(thread_id), Some(message_id)) = (
            self.id_customer_thread.as_ref(),
            self.id_customer_message.as_ref(),
        ) {
            if !thread_id.is_empty() && !message_id.is_empty() {
                self.delete_prestashop_note().await?;
            } else {
                info!("Thread ID or Message ID is empty: {thread_id:?} / {message_id:?}");
                let delete_res: Option<Record> = DATABASE
                    .delete((TASK_NOTE_TABLE, id.key().to_string()))
                    .await?;
                info!("delete_res: {delete_res:?}");
            }
        } else {
            info!("Not deleting prestashop note, there is either no thread id or no message id");
            let delete_res: Option<Record> = DATABASE
                .delete((TASK_NOTE_TABLE, id.key().to_string()))
                .await?;
            info!("Deleted note: {:?}", delete_res);
        }
        Ok(())
    }

    async fn delete_prestashop_note(&mut self) -> Result<(), Error> {
        if let Some(cust_msg_id) = self.id_customer_message.as_ref() {
            if !cust_msg_id.is_empty() {
                let prestashop = Prestashop::default();
                let delete_result = prestashop
                    .delete_resource_wasm("customer_messages", &cust_msg_id.clone())
                    .await?;

                info!("Delete Result for customer message: {delete_result:?}");

                let delete_res: Option<Record> = DATABASE
                    .query("DELETE task_note WHERE id_customer_message == $id_customer_message")
                    .bind(("id_customer_message", cust_msg_id.clone()))
                    .await?
                    .take(0)?;

                info!("Deleted note: {:?}", delete_res);
            }
        }
        Ok(())
    }
}

pub trait GetRec {
    
}


impl EmployeeHelper for Employee {
    async fn find_user(&mut self) -> Result<Option<User>, Error> {
        DATABASE.set("email", self.email.clone()).await?;
        let usr: Option<User> = DATABASE
            .query("SELECT * FROM user WHERE email == $email")
            .await?
            .take(0)?;
        debug!("user: {:?}", usr);
        Ok(usr)
    }

    async fn get_employee_from_id(&mut self, id_employee: &str) -> Result<Employee, Error> {
        let api_call = Prestashop::default();

        if id_employee != "0" && id_employee != "" {
            let employee: Employee = api_call
                .request_subresources_by_id("employees", "employee", &id_employee)
                .await?;

            Ok(Employee {
                email: employee.email.clone(),
                initials: employee.initials.clone(),
                firstname: employee.firstname.clone(),
                id_store: employee.id_store.clone(),
                lastname: employee.lastname.clone(),
                ..Default::default() // ..self.clone()
            })
        } else {
            Ok(Employee::default())
        }
    }

    async fn get_my_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[id_employee_sales_rep]", &mut self.id);
        query.insert("filter[id_store]", &mut self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "0,20");
        query.insert("output_format", "JSON");

        let orders: Vec<prestashop_schema::Order> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }

    async fn get_services_in_my_store(&mut self) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[id_store]", &mut self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "0,20");
        query.insert("output_format", "JSON");

        let orders: Vec<prestashop_schema::Order> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }

    async fn get_my_return_for_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error> {
        let mut api_call = Prestashop::default();
        api_call.display = "";
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "5");
        query.insert("output_format", "JSON");

        let order_details: Vec<prestashop_schema::OrderDetails> = api_call
            .request_resources_wasm("order_details", query.clone())
            .await?;

        let mut orders_vec = Vec::new();

        for order in order_details.iter() {
            let order_details: prestashop_schema::OrderDetails = api_call
                .request_subresources_by_id_wasm("order_details", "order_detail", &order.id)
                .await?;

            if let Some(id) = order_details.id_order {
                let mut order_query: HashMap<&str, &str> = HashMap::new();

                order_query.insert("sort", "[id_DESC]");
                order_query.insert("output_format", "JSON");

                let order: prestashop_schema::Order = api_call
                    .request_subresources_by_id_wasm("orders", "order", &id)
                    .await?;

                orders_vec.push(order);
            }
        }

        Ok(orders_vec)
    }

    async fn get_my_store_return_for_services(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "5");
        query.insert("output_format", "JSON");

        let order_details: Vec<prestashop_schema::OrderDetails> = api_call
            .request_resources_wasm("order_details", query.clone())
            .await?;

        let mut orders_vec = Vec::new();

        for order in order_details.iter() {
            let order_details: prestashop_schema::OrderDetails = api_call
                .request_subresources_by_id_wasm("order_details", "order_detail", &order.id)
                .await?;

            if let Some(id) = order_details.id_order {
                let mut order_query: HashMap<&str, &str> = HashMap::new();

                order_query.insert("sort", "[id_DESC]");
                order_query.insert("output_format", "JSON");
                order_query.insert("filter[id_store]", &mut self.id_store);
                order_query.insert("filter[id_order_type]", "2");

                let order: prestashop_schema::Order = api_call
                    .request_subresources_by_id_wasm("orders", "order", &id)
                    .await?;

                orders_vec.push(order);
            }
        }

        Ok(orders_vec)
    }
}

impl UserHelper for User {
    async fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[email]", &mut self.email);
        query.insert("output_format", "JSON");

        let employee: prestashop_schema::Employee = api_call
            .find_resource_wasm("employees", query.clone())
            .await?;
        Ok(employee)
    }

    async fn save_user_ui_layout(&mut self) -> Result<(), Error> {

        // info!(
        //     "User Settings to apply: {user_settings:?}\nTo User: {:?}",
        //     self.id.clone()
        // );

        match DATABASE
            .query("UPDATE $auth.id SET user_settings.ui_layout = $settings")
            .bind(("settings", self.user_settings.clone().unwrap().ui_layout))
            .await
        {
            Ok(res) => info!("Result: {res:?}"),
            Err(e) => info!("Error updating User Settings: {e:?}"),
        }
        Ok(())
    }

    // async fn save_theme_config(theme: ThemeConfig) -> Result<(), Error> {
    //     match DATABASE 
    //         .query("UPDATE $auth.id SET user_settings.color_scheme = $color_settings")
    //         .bind(("color_settings", theme.clone()))
    //         .await 
    //     {
    //         Ok(res) => info!("Res: {res:?}"),
    //         Err(e) => info!("Error updating User Settings: {e:?}"),
    //     }
    //     Ok(())
    // }

    fn get_odoo_store_number(&mut self) -> Result<u64, Error> {
        let store = match self.store {
            Store::RIV => 76,
            Store::LTN => 73,
            Store::MUR => 74,
            Store::AF => 72,
            Store::WJ => 78,
            Store::ORE => 75,
            Store::SAN => 77,
        };
        Ok(store)
    }

    fn get_store_from_odoo_id(&mut self) -> Result<Store, Error> {
        let store = match self.get_odoo_store_number()? {
            76 => Store::RIV,
            73 => Store::LTN,
            74 => Store::MUR,
            78 => Store::WJ,
            75 => Store::ORE,
            72 => Store::AF,
            77 => Store::SAN,
            _ => Store::RIV,
        };
        Ok(store)
    }
}

#[async_trait(?Send)]
impl ComputerDataHelper for ComputerData {
    async fn associate_to_service(&mut self) -> Result<prestashop_schema::ServiceOrder, Error> {
        todo!()
    }

    async fn find_associated_tickets(&mut self) -> Result<Vec<TicketData>, Error> {
        todo!()
    }

    async fn find_associated_client(&mut self) -> Result<ConnectedClient, Error> {
        todo!()
    }

    async fn find_associated_tasks(&mut self) -> Result<Vec<TaskPayload>, Error> {
        todo!()
    }

    async fn find_associated_customer(&mut self) -> Result<CustomerData, Error> {
        DATABASE
            .set("cust_id", self.customer.clone().unwrap())
            .await?;
        let customer: Option<CustomerData> = DATABASE
            .query("SELECT * FROM customer WHERE id == $cust_id")
            .await?
            .take(0)?;
        debug!("customer: {:?}", customer);
        Ok(customer.unwrap())
    }

    async fn find_associated_prestashop_orders(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error> {
        todo!()
    }

    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error> {
        let cust = self.find_associated_customer().await?;
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[email]", &cust.email);
        query.insert("output_format", "JSON");

        let customer: prestashop_schema::Customer = api_call
            .find_resource_wasm("customers", query.clone())
            .await?;
        Ok(customer)
    }
}

fn convert_date_string(input: &str) -> Result<String, chrono::ParseError> {
    // Define the input format as per the provided string.
    let format = "%Y-%m-%d %H:%M:%S";

    // Parse the input string into a NaiveDateTime (which doesn't include timezone information).
    let naive_dt = NaiveDateTime::parse_from_str(input, format)?;

    // Convert the NaiveDateTime to a DateTime<Utc> with the assumption that it is in UTC.
    let datetime_utc = Utc.from_utc_datetime(&naive_dt);

    // Format the DateTime<Utc> to the desired ISO 8601 string with milliseconds.
    let result = datetime_utc.to_rfc3339();

    Ok(result)
}

/// Parses the username from an email address
fn parse_email_user(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub date_add: String,
    pub id: String,
    pub date_upd: String,
}


impl From<u64> for Store {
    fn from(value: u64) -> Self {
        match value {
            76 => Store::RIV,
            73 => Store::LTN,
            74 => Store::MUR,
            78 => Store::WJ,
            75 => Store::ORE,
            72 => Store::AF,
            77 => Store::SAN,
            _ => Store::RIV,
        }
    }
}

// impl From<&mut u64> for &str {
//     fn from(value: u64) -> Self {
//         match value {
//             76 => Store::RIV.as_str(),
//             73 => Store::LTN.as_str(),
//             74 => Store::MUR.as_str(),
//             78 => Store::WJ.as_str(),
//             75 => Store::ORE.as_str(),
//             72 => Store::AF.as_str(),
//             77 => Store::SAN.as_str(),
//             _ => Store::RIV.as_str(),
//         }
//     }
// }

// pub trait StoreHelper {
//     fn store_id_to_str(&mut self) -> 
// }
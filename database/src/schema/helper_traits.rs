#![allow(async_fn_in_trait)]
use super::{
    prestashop_schema::{self, CustomerMessage, CustomerThread, Employee, Prestashop, PrestashopPayload}, ComputerData, ConnectedClient, CustomerData, Notification, Record, Store, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User, TASK_NOTE_TABLE
};
use crate::{schema::{utilities::query_user_from_email, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE}, PlatformSpawner, Spawner, DATABASE};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use crate::schema::deserializer::deserialize_to_string;
use std::{collections::HashMap, fmt::Debug};
use chrono::{NaiveDateTime, SecondsFormat, TimeZone, Utc};
use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use structdiff::StructDiff;
use surrealdb::RecordId;
use log::{debug, info, warn};
use serde_json::Value;
use regex::Regex;

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
    async fn get_my_services_in_repair(&mut self) -> Result<Vec<OrderNumber>, Error>;
    async fn get_all_my_services(&mut self) -> Result<Vec<OrderNumber>, Error>;
    /// Get all orders in my store given Employee info -> id_location
    async fn get_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<OrderNumber>, Error>;
    /// Get all Orders of which are my Return For Service's
    async fn get_my_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all Orders of which are In the given status
    async fn get_services_by_status(&mut self, status: &str, start_idx: i32, offset: i32) -> Result<Vec<OrderNumber>, Error>;
    /// Get all services in my store
    async fn get_all_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<OrderNumber>, Error>;
    /// Get all Return For Service's in my store
    async fn get_my_store_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get Employee from ID
    async fn get_employee_from_id(&mut self, id_employee: &str) -> Result<Employee, Error>;
    /// Convert an order into a PrestashopPayload
    async fn to_prestashop_payload(service_number: &str) -> Result<prestashop_schema::PrestashopPayload, Error> ;
}

/// A trait for assisting with operations involving the `User` struct.
pub trait UserHelper {
    /// Finds and retrieves the associated Employee record based on the User information.
    ///
    /// # Returns
    /// - `Ok(Employee)` on success, where `Employee` is a struct representing the employee record.
    /// - `Err(Error)` if the employee cannot be found or an error occurs during the operation.
    async fn find_employee_by_email(&mut self) -> Result<prestashop_schema::Employee, Error>;

    /// Saves the user settings to the database or persistent storage.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(Error)` if an error occurs while saving the settings.
    async fn save_mastertech_ui_layout(&mut self, settings: Value) -> Result<(), Error>;


    /// Saves the user settings to the database or persistent storage.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(Error)` if an error occurs while saving the settings.
    async fn save_mtechserver_ui_layout(&mut self, settings: Value) -> Result<(), Error>;

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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct OrderNumber {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String
}

/// A trait for managing operations and data related to task note payloads.
pub trait TaskNotePayloadHelper: Send {
    /// Creates a task note in the Prestashop system.
    ///
    /// # Returns
    /// - `Ok(PrestaResourceResponse)` on successful creation.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    async fn create_customer_message(&mut self) -> Result<PrestaResourceResponse, anyhow::Error>
    where
        anyhow::Error: Send;

    /// Creates a customer thread in Prestashop so we can create messages for it.
    ///
    /// # Returns
    /// - `Ok(PrestaResourceResponse)` on successful creation.
    /// - `Err(anyhow::Error)` if an error occurs during the creation.
    async fn create_customer_thread(&mut self) -> Result<PrestaResourceResponse, Error>
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
    async fn handle_note_creation(&mut self, private: bool) -> Result<(), anyhow::Error>
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
    async fn update_user_info_if_needed(&mut self) -> Result<(), anyhow::Error>
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

    /// Retrieves the thread ID based on an order NUMBER.
    ///
    /// # Returns
    /// - `Ok(Vec<TaskNotePayload>)` containing the notes from an order.
    /// - `Err(Error)` if the thread ID cannot be found or an error occurs.
    async fn get_notes_from_service_number(&mut self, service_number: &str) -> Result<Vec<TaskNotePayload>>;

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
    async fn modify_note(&mut self) -> Result<(), Error>;

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
    async fn handle_note_creation(&mut self, private: bool) -> Result<(), anyhow::Error> {
        if self.created_at.is_empty() {
            self.update_task_note_with_current_time().await?;
        }

        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            self.get_thread_id_from_order().await?
        };

        if self.id_customer_message.is_none()
            && !id_customer_thread.is_empty()
            && self.id_employee.is_some()
            && !private
        {
            self.id_customer_thread = Some(id_customer_thread);
            // Is this sent from the website or mastertech?
            info!("helper_traits -> Sent from website, {:?} - {:?}", self.id_customer_thread, self.id_employee);
            let response = self.create_customer_message().await?;
            info!("helper_traits -> handle_note_creation -> Before struct diffing TaskNotePayload: {:?}", self.clone());
            // Update task note with Prestashop details
            let id = if self.id.key().to_string().is_empty() {
                let task_note_default = TaskNotePayload::default();
                info!("helper_traits -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
                task_note_default.id
            } else {
                if !response.id.to_string().is_empty() {
                    let id = RecordId::from((TASK_NOTE_TABLE, response.id.to_string().clone()));
                    info!("helper_traits -> handle_note_creation -> id is not empty, creating with cust message id: {id:?}");
                    id
                } else {
                    let task_note_default = TaskNotePayload::default();
                    info!("helper_traits -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
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

            info!("helper_traits -> handle_note_creation -> After struct diffing TaskNotePayload: {:?}", self.clone());

            self.create_task_note_in_db().await?;

            self.update_user_info_if_needed().await?;

        } else if id_customer_thread.is_empty() && self.service_number.is_some() {
            let create_thread_response = self.create_customer_thread().await?;
            info!("helper_traits -> handle_note_creation -> We do NOT have a customer thread ID, and we HAVE a service number, creating thread.");
            self.id_customer_thread = Some(create_thread_response.id.clone());
            self.created_at = create_thread_response.date_add;
            if self.id_customer_message.is_none()
                && !create_thread_response.id.is_empty()
                && self.id_employee.is_some()
            {
                // Is this sent from the website or mastertech?
                info!("helper_traits -> Sent from website, {:?} - {:?}", self.id_customer_thread, self.id_employee);
                let response = self.create_customer_message().await?;
                info!("helper_traits -> handle_note_creation -> Before struct diffing TaskNotePayload: {:?}", self.clone());
                // Update task note with Prestashop details
                let id = if self.id.key().to_string().is_empty() {
                    let task_note_default = TaskNotePayload::default();
                    info!("helper_traits -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
                    task_note_default.id
                } else {
                    if !response.id.to_string().is_empty() {
                        let id = RecordId::from((TASK_NOTE_TABLE, response.id.to_string().clone()));
                        info!("helper_traits -> handle_note_creation -> id is not empty, creating with cust message id: {id:?}");
                        id
                    } else {
                        let task_note_default = TaskNotePayload::default();
                        info!("helper_traits -> handle_note_creation -> ID is empty, assigning a new id: {:?}", task_note_default.id);
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

                info!("helper_traits -> handle_note_creation -> After struct diffing TaskNotePayload: {:?}", self.clone());

                self.create_task_note_in_db().await?;

                self.update_user_info_if_needed().await?;
            }
        } else if id_customer_thread.is_empty() && self.service_number.is_none() {
            info!("helper_traits -> handle_note_creation -> We do NOT have a customer thread ID, and we do NOT have a service number. creating a regular task note. {:?}", self.clone());
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
            info!("helper_traits -> handle_note_creation -> This is an odd case... {:?}", self.clone());
            let upsert_note_record: Option<TaskNotePayload> = DATABASE
                .upsert(self.id.clone())
                .content(self.clone())
                .await?;

            log::info!("upsert_note_record: {upsert_note_record:?}");
        }
        self.check_tagged_user_in_note().await?;

        Ok(())
    }

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
                info!("helper_traits -> check_tagged_user_in_note -> There is an ID, and there IS a tagged user: {id:?} / {tagged_user:?}");
                let task_name: Option<String> = DATABASE
                    .query("SELECT VALUE task_name FROM task WHERE id == $task_id")
                    .bind(("task_id", task_id.clone()))
                    .await?
                    .take(0)?;

                info!("helper_traits -> check_tagged_user_in_note -> Task Name: {:?}", task_name);
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
        info!("helper_traits -> Creating notification: {:?}", notification);
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

    async fn update_user_info_if_needed(&mut self) -> Result<(), Error> {
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
        self.created_at = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        info!("helper_traits -> Created_at was empty, now it is {:?}", self.created_at);
        Ok(())
    }

    async fn update_task_note_fields(&mut self) -> Result<(), Error> {
        // Logic to update task note fields
        Ok(())
    }

    async fn create_customer_thread(&mut self) -> Result<PrestaResourceResponse, Error> {
        if self.service_number.is_none() {
            return Err(anyhow::anyhow!("service number is empty")).into();
        };
        
        let presta_api = Prestashop::default();

        let order: prestashop_schema::Order = presta_api
            .request_subresources_by_id_wasm("orders", "order", &self.service_number.clone().unwrap_or_default())
            .await?;

        let id_customer = order.id_customer;

        Ok(
            presta_api.create_customer_thread(
                &self.service_number.clone().unwrap_or_default(), 
                &id_customer
            ).await?
        )
    }

    async fn create_customer_message(&mut self) -> Result<PrestaResourceResponse, Error> {
        let thread_id = self.get_thread_id_from_order().await?;
        let id_employee = self.id_employee.as_deref().unwrap_or("");

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

    async fn get_order_by_task_id(&mut self) -> Result<String> {
        if let Some(id) = self.task_id.clone() {
            // I cannot do this at the moment because GetAssociatedData requires + Send
            // let task: TaskPayload = id.get_associated_data::<TaskPayload>().await?;
            // let service_number = task.service_number;
            let service_number: Option<String> = DATABASE
                .query("SELECT VALUE service_number FROM task WHERE id == $task_id")
                .bind(("task_id", id.clone()))
                .await?
                .take(0)?;
            info!("Order number pulled from service_number: {service_number:?} using task id: {id:?}");
            Ok(service_number.unwrap_or_default())
        } else {
            info!("helper_traits -> No order number found");
            Ok(String::new())
        }
    }

    async fn get_thread_id_from_order(&mut self) -> Result<String> {
        let mut threads = Vec::new();
        if let Ok(service_number) = self.get_order_by_task_id().await {
            info!("helper_traits -> Calling API for thread ID");
            
            if !service_number.is_empty() {
                let api_call = Prestashop::default();
                let mut query: HashMap<&str, &str> = HashMap::new();
    
                query.insert("filter[id_order]", &service_number);
                query.insert("output_format", "JSON");

                let customer_threads: Vec<CustomerThread> = api_call
                    .request_resources_wasm("customer_threads", query.clone())
                    .await?;
                
                info!("helper_traits -> get_thread_id_from_order -> Got customer threads: {customer_threads:?}");
                if customer_threads.is_empty() && self.id_customer_thread.is_none() && self.service_number.is_some() {

                    let create_thread_response = self.create_customer_thread().await?;
                    info!("helper_traits -> handle_note_creation -> We do NOT have a customer thread ID, and we HAVE a service number, creating thread.");
                    self.id_customer_thread = Some(create_thread_response.id);
                    self.created_at = create_thread_response.date_add;
                }

                for thread in customer_threads {
                    for msg in thread.associations.customer_messages.iter() {
                        info!("helper_traits -> get_thread_id_from_order -> Checking if msg ID exists in database: {:?}", &msg);
                        let exists = self.check_existing_note_record(&msg.id).await?;
                        if exists.is_none() {
                            info!("helper_traits -> get_thread_id_from_order -> WE NEED TO CREATE THIS MESSAGE IN OUR DATABASE: {msg:?}");
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
                                    // Into::<surrealdb::sql::Datetime>::into(convert_date_string(&customer_message.date_add)?).to_string()
                                    created_at: convert_date_string(&customer_message.date_add)?,
                                    note: customer_message.message,
                                    username: parse_email_user(&employee.email).to_string(),
                                    user: if let Some(usr) = employee.find_user().await? { Some(usr.id) } else { None },
                                    service_number: Some(service_number.to_string())
                                };

                                info!("helper_traits -> get_thread_id_from_order -> Creating a new task_note: {task_note:?}");

                                match task_note.create_task_note_in_db().await {
                                    Ok(_) => log::info!("Created task note: {task_note:?}"),
                                    Err(e) => log::info!("Error creating task note: {e:?}"),
                                }
                            } else {
                                info!("helper_traits -> get_thread_id_from_order -> Customer Message contains some bad data: {customer_message:#?}");
                            }
                        } else {
                            info!("helper_traits -> get_thread_id_from_order -> Message already exists: {:?}", exists);
                        }
                    }
                    threads.push(thread);
                }
            } else {
                info!("helper_traits -> Service number is empty. not querying Presta {service_number:?}");
                // return Err(anyhow::anyhow!("service number is empty")).into();
            }
        } else {
            info!("helper_traits -> Error getting order number from task id\nIs there a task ID??");
            return Err(anyhow::anyhow!("helper_traits -> Error getting order number from task id\nIs there a task ID??")).into();
        }

        // Extract the first customer thread ID if available
        let id_customer_thread = threads
            .iter()
            .filter_map(|m| Some(m.id.clone()))
            .next()
            .unwrap_or_default();

        info!("helper_traits -> Customer thread id from order: {id_customer_thread:?}");

        Ok(id_customer_thread)
    }

    async fn get_notes_from_service_number(&mut self, service_number: &str) -> Result<Vec<TaskNotePayload>> {
        let mut notes = Vec::new();
        info!("helper_traits -> get_notes_from_service_number -> Calling get_notes_from_service_number");
        
        if !service_number.is_empty() {
            let api_call = Prestashop::default();
            let mut query: HashMap<&str, &str> = HashMap::new();

            query.insert("filter[id_order]", &service_number);
            query.insert("output_format", "JSON");

            let customer_threads: Vec<CustomerThread> = api_call
                .request_resources_wasm("customer_threads", query.clone())
                .await?;
            
            info!("helper_traits -> get_notes_from_service_number -> Got customer threads: {customer_threads:?}");
            for thread in customer_threads {
                for msg in thread.associations.customer_messages.iter() {
                    info!("helper_traits -> get_notes_from_service_number -> Checking if msg ID exists in database: {:?}", &msg);
                    let exists = self.check_existing_note_record(&msg.id).await?;
                    if exists.is_none() {
                        info!("helper_traits -> get_notes_from_service_number -> WE NEED TO CREATE THIS MESSAGE IN OUR DATABASE: {msg:?}");
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
                            created_at: convert_date_string(&customer_message.date_add)?,
                            note: customer_message.message,
                            username: parse_email_user(&employee.email).to_string(),
                            service_number: Some(service_number.to_string()),
                            user: if let Some(usr) = employee.find_user().await? { Some(usr.id) } else { None },
                        };
                        warn!("helper_traits -> get_notes_from_service_number -> Creating a new task_note: {task_note:?}");

                        let note: Option<Record> = DATABASE
                            .query("CREATE task_note CONTENT $task_note")
                            .bind(("task_note", task_note.clone()))
                            .await?
                            .take(0)?;

                        notes.push(task_note);

                        warn!("helper_traits -> get_notes_from_service_number -> Created note: {note:?}");
                    } else {
                        warn!("helper_traits -> get_notes_from_service_number -> Message already exists: {:?}", exists);
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
            info!("helper_traits -> get_notes_from_service_number -> Order number is still empty? {service_number:?}");
        }

        Ok(notes)
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
            .await?
            .take(0)?;


        info!("helper_traits -> Message existence query results: {query_results:?}");
        
        for note in query_results.iter() {
            warn!("helper_traits -> existing note Task ID: {:?}", note.task_id);
            warn!("helper_traits -> self.note Task ID: {:?}", self.task_id);
            if let (Some(existing_task_id), Some(task_id)) = (&note.task_id, &self.task_id) {
                if existing_task_id != task_id {
                    info!("helper_traits -> UPDATE task_note SET task_id = {task_id:?} WHERE id == {:?}", note.id);
                    let res: Option<TaskNotePayload> = DATABASE
                        .query("UPDATE task_note SET task_id = $new_id WHERE id == $id")
                        .bind(("id", note.id.clone()))
                        .bind(("new_id", task_id.clone()))
                        .await?
                        .take(0)?;

                    log::info!("Task note already exists: {res:#?}, but it should now have a new task id: {task_id:?}");
                }
            }

            warn!("helper_traits -> note.id != self.id: {:?} {:?}", note.id, self.id);
            
            if note.id != self.id {
                return Ok(Some(note.id.clone()));
            }
        }
        Ok(None)
    }

    async fn modify_note(&mut self) -> Result<(), Error> {
        let id_customer_thread = if let Some(thread_id) = self.id_customer_thread.as_ref() {
            thread_id.clone()
        } else {
            self.get_thread_id_from_order().await?
        };

        let id_customer_message = self.id_customer_message.clone();
        let id_employee = self.id_employee.clone();
        // REGULAR TASK NOTE / NOT A PRESTASHOP NOTE
        if id_customer_message.is_none() {
            let upsert_note_record: Option<TaskNotePayload> = DATABASE
                .upsert(self.id.clone())
                .content(self.clone())
                .await?;

            log::info!("upsert_note_record: {upsert_note_record:?}");
        }
        // PRESTASHOP NOTE
        else if id_customer_message.is_some() && !id_customer_thread.is_empty() && id_employee.is_some() {
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
            return Err(
                anyhow::anyhow!(
                    "One of (id_customer_message, id_customer_thread, id_employee) is empty\n{:?}", self.clone()
                )
            ).into();
        }

        Ok(())
    }

    async fn delete_note(&mut self) -> Result<(), Error> {
        let id = self.id.clone();
        info!("helper_traits -> deleting id: {:?}", &id);
        if let (Some(thread_id), Some(message_id)) = (
            self.id_customer_thread.as_ref(),
            self.id_customer_message.as_ref(),
        ) {
            if !thread_id.is_empty() && !message_id.is_empty() {
                self.delete_prestashop_note().await?;
            } else {
                info!("helper_traits -> Thread ID or Message ID is empty: {thread_id:?} / {message_id:?}");
                let delete_res: Option<Record> = DATABASE
                    .delete((TASK_NOTE_TABLE, id.key().to_string()))
                    .await?;
                info!("helper_traits -> delete_res: {delete_res:?}");
            }
        } else {
            info!("helper_traits -> Not deleting prestashop note, there is either no thread id or no message id");
            let delete_res: Option<Record> = DATABASE
                .delete((TASK_NOTE_TABLE, id.key().to_string()))
                .await?;
            info!("helper_traits -> Deleted note: {:?}", delete_res);
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

                info!("helper_traits -> Delete Result for customer message: {delete_result:?}");

                let delete_res: Option<Record> = DATABASE
                    .query("DELETE task_note WHERE id_customer_message == $id_customer_message")
                    .bind(("id_customer_message", cust_msg_id.clone()))
                    .await?
                    .take(0)?;

                info!("helper_traits -> Deleted note: {:?}", delete_res);
            }
        }
        Ok(())
    }
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
                .request_subresources_by_id_wasm("employees", "employee", &id_employee)
                .await?;

            Ok(Employee {
                email: employee.email.clone(),
                initials: employee.initials.clone(),
                firstname: employee.firstname.clone(),
                id_store: employee.id_store.clone(),
                lastname: employee.lastname.clone(),
                id: employee.id.clone(),
                ..Default::default() // ..self.clone()
            })
        } else if !self.email.is_empty() {
            let mut user = User::default();
            user.email = self.email.clone();
            Ok(user.find_employee_by_email().await?)
        } else {
            Ok(Employee::default())
        }
    }

    async fn get_my_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error> {
        let mut api_call = Prestashop::default();
        api_call.display = "";
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
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

    async fn get_my_store_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
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

    async fn get_my_services_in_repair(&mut self) -> Result<Vec<OrderNumber>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_employee_sales_rep]", &self.id);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("filter[current_state]", "30");
        query.insert("sort", "[id_DESC]");
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<OrderNumber> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        info!("helper_traits -> Orders list: {orders:?}");
        Ok(orders)
    }

    async fn get_all_my_services(&mut self) -> Result<Vec<OrderNumber>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_employee_sales_rep]", &self.id);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("filter[current_state]", "30");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "20");
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<OrderNumber> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        info!("helper_traits -> Orders list: {orders:?}");
        Ok(orders)
    }

    async fn get_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<OrderNumber>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<OrderNumber> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }

    async fn get_all_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<OrderNumber>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<OrderNumber> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }
 
    async fn get_services_by_status(&mut self, status: &str, start_idx: i32, offset: i32) -> Result<Vec<OrderNumber>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);

        info!("helper_traits -> Pagination: {pagination}");

        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("filter[current_state]", status);
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<OrderNumber> = api_call
            .request_resources_wasm("orders", query.clone())
            .await.context("Pulling orders list")?;

        info!("helper_traits -> Orders list: {orders:?}");
        Ok(orders)
    }

    async fn to_prestashop_payload(service_number: &str) -> Result<prestashop_schema::PrestashopPayload, Error> {
        let mut api_call = Prestashop::default();
        let mut query = HashMap::new();
        info!("helper_traits -> Pulling order {service_number}");
        query.insert("filter[id]", service_number);
        query.insert("output_format", "JSON");
        // api_call.display = "[id,id_address_invoice,id_customer,current_state,date_add,id_employee_sales_rep,id_employee_split_rep,id_store,associations]";
    
        let customer_threads: Vec<prestashop_schema::CustomerThread> = api_call
            .request_resources_wasm("customer_threads", query.clone())
            .await?;
    
        let mut customer_messages: Vec<prestashop_schema::CustomerMessage> = Vec::new();
    
        if !customer_threads.is_empty() {
            for thread in customer_threads.iter() {
                for msg in thread.associations.customer_messages.iter() {
                    let msg =  api_call
                        .request_subresources_by_id_wasm(
                            "customer_messages",
                            "customer_message",
                            msg.id.as_str(),
                        )
                        .await?;
                    customer_messages.push(msg)
                }
            }
        }
    

        let order: prestashop_schema::Order = api_call
            .find_resource_wasm("orders", query.clone())
            .await.context("Pulling order")?;

        api_call.display = "full";
        if order.id_customer.is_empty() 
        {
            return Err(anyhow::anyhow!("order.id_customer is empty")).into();
        }

        api_call.display = "[id,id_store,lastname,firstname,email,initials]";

        let sales_rep: Option<Employee>  = if !order.id_employee_sales_rep.eq("checkinshelf") && !order.id_employee_sales_rep.eq("0"){
            let mut new_query = query.clone();
            new_query.clear();
            new_query.insert("filter[id]", &order.id_employee_sales_rep);
            new_query.insert("output_format", "JSON");
            Some(
                api_call
                .find_resource_wasm(
                    "employees",
                    new_query
                )
                .await.context("Pulling employee")?
            )
        } else {
            let mut emp = Employee::default();
            emp.firstname = "CheckInShelf".to_string();
            Some(emp)
        };

        let split_rep: Option<Employee> = if !order.id_employee_split_rep.eq("0") {
            let mut new_query = query.clone();
            new_query.clear();
            new_query.insert("filter[id]", &order.id_employee_split_rep);
            new_query.insert("output_format", "JSON");
            let employee_2: Employee = api_call
                .find_resource_wasm(
                    "employees",
                    new_query
                )
                .await
                .context("Pulling split rep")?;

            info!("helper_traits -> employee: {sales_rep:#?}");
            Some(employee_2)
        } else {
            None
        };

        let cust: prestashop_schema::Customer = if order.id_employee_sales_rep.eq("0") {
            let mut cust = prestashop_schema::Customer::default();
            cust.firstname = "Checkin".to_string();
            cust.lastname = "Shelf".to_string();
            cust
        } else {
            api_call.display = "[lastname,firstname,email]";
            let mut new_query = query.clone();
            new_query.clear();
            new_query.insert("filter[id]", &order.id_customer);
            new_query.insert("output_format", "JSON");
            api_call
                .find_resource_wasm(
                    "customers", 
                    new_query
                )
                .await.context("Pulling customer")?
        };
        
        let customer = CustomerData {
            id: RecordId::from((
                CUSTOMER_TABLE.to_string(),
                order.id_customer.clone(),
            )),
            cust_code: order.id_customer.clone(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            // phone_number: address.phone.clone().to_string(),
            email: cust.email,
            ..Default::default()
        };

        Ok(
            prestashop_schema::PrestashopPayload {
                customer,
                order,
                sales_rep,
                split_rep,
                customer_threads,
                customer_messages,
                ..Default::default()
            }
        )
    }
}

impl UserHelper for User {
    async fn find_employee_by_email(&mut self) -> Result<prestashop_schema::Employee, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[email]", &mut self.email);
        query.insert("output_format", "JSON");

        let employee: prestashop_schema::Employee = api_call
            .find_resource_wasm("employees", query.clone())
            .await?;
        Ok(employee)
    }

    async fn save_mastertech_ui_layout(&mut self, settings: Value) -> Result<(), Error>{
        info!("helper_traits -> Settings for MASTERTECH: {:?}", settings.clone());
        match DATABASE
            .query("UPDATE $auth.id SET user_settings.ui_layout.mastertech = $settings")
            .bind(("settings", settings))
            .await
        {
            Ok(res) => info!("helper_traits -> Result: {res:?}"),
            Err(e) => info!("helper_traits -> Error updating User Settings: {e:?}"),
        }
        Ok(())
    }
    
    async fn save_mtechserver_ui_layout(&mut self, settings: Value) -> Result<(), Error>{
        info!("helper_traits -> Settings for MTECHSERVER: {:?}", settings.clone());
        match DATABASE
            .query("UPDATE $auth.id SET user_settings.ui_layout.mtechserver = $settings")
            .bind(("settings", settings))
            .await
        {
            Ok(res) => info!("helper_traits -> Result: {res:?}"),
            Err(e) => info!("helper_traits -> Error updating User Settings: {e:?}"),
        }
        Ok(())
    }

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

#[derive(Debug, Serialize, Deserialize)]
pub struct PrestaResourceResponse {
    pub date_add: String,
    pub id: String,
    pub date_upd: String,
    // pub service_number: String,
}

#[async_trait(?Send)]
pub trait PrestashopPayloadHelper<'a>: Send + Sync  {
    async fn get_prestashop_payload(&mut self, service_number: &str) -> Result<PrestashopPayload, Error>;
    async fn get_customer_threads(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_customer_messages(&mut self, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_order(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_employee(&mut self, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_customer(&mut self, prestashop_api: &Prestashop) -> Result<(), Error>;
}

#[async_trait(?Send)]
impl <'a>PrestashopPayloadHelper<'a> for PrestashopPayload {
    async fn get_prestashop_payload(&mut self, service_number: &str) -> Result<Self, Error> {
        let prestashop_api = Prestashop::default();
        self.get_customer_threads(&service_number, &prestashop_api).await?;
        self.get_customer_messages(&prestashop_api).await?;
        self.get_order(&service_number, &prestashop_api).await?;
        self.get_employee(&prestashop_api).await?;
        self.get_customer(&prestashop_api).await?;

        Ok(self.clone())
    }
    async fn get_customer_threads(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error> {
        if !self.customer_threads.is_empty() {
            let mut query = HashMap::new();
            query.insert("filter[id_order]", service_number);
            query.insert("output_format", "JSON");
            self.customer_threads = prestashop_api
                .request_resources_wasm("customer_threads", query.clone())
                .await?;
        }
        Ok(())
    }
    async fn get_customer_messages(&mut self, prestashop_api: &Prestashop) -> Result<(), Error> {
        for thread in self.customer_threads.iter() {
            for msg in thread.associations.customer_messages.iter() {
                let msg =  prestashop_api
                    .request_subresources_by_id_wasm(
                        "customer_messages",
                        "customer_message",
                        &msg.id,
                    )
                    .await?;
                self.customer_messages.push(msg)
            }
        }
        Ok(())
    }
    async fn get_order(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error> {
        self.order = prestashop_api.request_subresources_by_id_wasm("orders", "order", service_number).await?;
        Ok(())
    }
    async fn get_employee(&mut self, prestashop_api: &Prestashop) -> Result<(), Error> {
        let sales_rep: Option<Employee> = if !self.order.id_employee_sales_rep.eq("checkinshelf") && !self.order.id_employee_sales_rep.eq("0") {
            //|| order.id_employee_sales_rep.len() != 0{
            let employee: Employee = prestashop_api
                .request_subresources_by_id_wasm(
                    "employees",
                    "employee",
                    &self.order.id_employee_sales_rep,
                )
                .await?;
    
            info!("helper_traits -> employee: {employee:#?}");
            Some(employee)
        } else {
            None
        };
    
        self.sales_rep = sales_rep;

        let split_rep: Option<Employee> = if !self.order.id_employee_split_rep.eq("0") {
            let employee_2: Employee = prestashop_api
                .request_subresources_by_id_wasm(
                    "employees",
                    "employee",
                    &self.order.id_employee_split_rep,
                )
                .await?;
    
            info!("helper_traits -> employee: {employee_2:#?}");
            Some(employee_2)
        } else {
            None
        };
        self.split_rep = split_rep;

        Ok(())
    }
    async fn get_customer(&mut self, prestashop_api: &Prestashop) -> Result<(), Error> {
        let cust: prestashop_schema::Customer = if self.order.id_employee_sales_rep.eq("0") {
            let mut cust = prestashop_schema::Customer::default();
            cust.firstname = "Checkin".to_string();
            cust.lastname = "Shelf".to_string();
            cust
        } else {
            prestashop_api
                .request_subresources_by_id_wasm("customers", "customer", &self.order.id_customer)
                .await.context("Pulling customer")?
        };

        let address: prestashop_schema::Address = prestashop_api
            .request_subresources_by_id_wasm("addresses", "address", &self.order.id_address_invoice)
            .await?;

        self.customer = CustomerData {
            id: RecordId::from((
                CUSTOMER_TABLE.to_string(),
                self.order.id_customer.clone(),
            )),
            cust_code: self.order.id_customer.clone(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            phone_number: address.phone.clone().to_string(),
            // phone_number_2: address.phone_mobile.clone().unwrap_or(0).to_string(),
            email: cust.email,
            ..Default::default()
        };

        Ok(())
    }
}


impl From<TaskPayload> for PrestashopPayload {
    fn from(_value: TaskPayload) -> Self {
        todo!()
    }
}

impl From<PrestashopPayload> for TaskPayload {
    fn from(value: PrestashopPayload) -> Self {
        let customer = &mut CustomerData::default();
        let ticket = &mut TicketPayload::default();
        let task = &mut TaskPayload::default();
        let mut task_notes = Vec::new();

        let service_details = value.order.associations.order_service.clone();
        let mut services: Vec<RecordId> = Vec::new();

        let sales_rep = value.sales_rep.clone().unwrap_or_default();
        let split_rep = value.split_rep.clone().unwrap_or_default();
        let email = parse_email_user(&sales_rep.email);
        let email_split_rep = parse_email_user(&split_rep.email);

        customer.id = value.customer.id.clone();
        customer.cust_code = value.customer.cust_code.clone();
        customer.email = value.customer.email.clone();
        customer.name = value.customer.name.clone();
        customer.phone_number = value.customer.phone_number.clone();
        ticket.salesman = email_split_rep.to_string();
        ticket.sales_rep = email.to_string();
        ticket.tech = email.to_string();
        info!(
            "Salesman: {:?}\nTech: {:?}",
            ticket.salesman.clone(),
            ticket.tech.clone()
        );
        ticket.customer = Some(customer.clone());
        ticket.checkin_rep = email.to_string();
        ticket.terms = value.order.payment.clone();
        ticket.ticket_total = value.order.total_products_wt.clone();
        ticket.doc_alias = value.order.order_type.clone();
        ticket.service_number = value.order.id.clone();
        ticket.id = RecordId::from((
            TICKET_TABLE.to_string(),
            ticket.service_number.clone(),
        ));
        task.id = RecordId::from((
            TASK_TABLE.to_string(),
            ticket.service_number.clone(),
        ));

        let (tx, rx) = crossbeam::channel::bounded::<User>(1);
        
        let employees = value
            .customer_messages
            .iter()
            .map(|msg| msg.id_employee.clone())
            .collect::<Vec<String>>();

        PlatformSpawner::spawn(async move {
            let res = async {
                for emp in employees.iter() {
                    let employee = Employee::default().get_employee_from_id(emp).await?;
                    let mut usr = User::default();
                    usr.email = employee.email;
                    let emp = usr.find_employee_by_email().await?;
                    tx.try_send(query_user_from_email(emp.email.clone()).await?)?;
                }
                
                Ok::<(), anyhow::Error>(())
            }.await;
            log::info!("Res: {res:?}");
        });

        let user = &mut User::default();

        if let Ok(usr) = rx.try_recv() { 
            *user = usr;
        }

        for msg in value.customer_messages.iter() {
            // let initials = if msg.id_employee 
            task_notes.push(TaskNotePayload {
                note: msg.message.clone(),
                id: RecordId::from((TASK_NOTE_TABLE, msg.id.clone())),
                task_id: Some(task.id.clone()),
                created_at:  match convert_date_string(&msg.date_add) {
                    Ok(date) => date,
                    Err(e) => {
                        log::info!("Parse error: {e:?}");
                        msg.date_add.clone()
                    },
                },
                id_customer_thread: Some(msg.id_customer_thread.clone()),
                id_customer_message: Some(msg.id.clone()),
                id_employee: Some(msg.id_employee.clone()),
                username: parse_email_user(&user.email).to_string(),
                user: Some(user.id.clone()),
                service_number: Some(ticket.service_number.clone()),
                // ..Default::default()
            })
        }
        task.task_note = task_notes;
        services.push(ticket.id.clone());
        
        if !service_details.is_empty() {
            if service_details.len() == 1 {
                let svc = service_details.get(0);
                if let Some(service) = svc {
                    ticket.checkin_notes = service.check_in_notes.clone();
                }
            } else {
                info!("helper_traits -> Theres a couple.... {:?}", service_details);
            }
        }

        task.service_ticket = Some(ticket.clone());

        task.task_name = format!(
            "{} - {}",
            &customer.name,
            ticket.service_number.clone()
        );
        task.clone()
    }
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

impl From<Store> for u64 {
    fn from(value: Store) -> Self {
        match value {
            Store::RIV => 76,
            Store::LTN => 73,
            Store::MUR => 74,
            Store::WJ => 78,
            Store::ORE => 75,
            Store::AF => 72,
            Store::SAN => 77
        }
    }
}


pub fn convert_date_string(input: &str) -> Result<String, chrono::ParseError> {
    // Define the input format as per the provided string.
    let format = "%Y-%m-%d %H:%M:%S";

    // Parse the input string into a NaiveDateTime (which doesn't include timezone information).
    let naive_dt = NaiveDateTime::parse_from_str(input, format)?;

    // Convert the NaiveDateTime to a DateTime<Utc> with the assumption that it is in UTC.
    let datetime_utc = Utc.from_utc_datetime(&naive_dt);

    // Format the DateTime<Utc> to the desired ISO 8601 string with milliseconds.
    let result = datetime_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    Ok(result)
}

/// Parses the username from an email address
pub fn parse_email_user(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}

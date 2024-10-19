use crate::DATABASE;

use super::{
    prestashop_schema::{self, CustomerMessage, Employee, Prestashop}, ComputerData, ConnectedClient, CustomerData, ExtendedSeb, Notification, Record, SpecialPartOrder, Store, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User, TASK_NOTE_TABLE
};
use anyhow::{Error, Result};
use async_trait::async_trait;
use log::{debug, info};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fmt::Debug};
use structdiff::StructDiff;
use regex::Regex;
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

/// A trait for assisting with operations involving the Employee struct
#[async_trait(?Send)]
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

/// A trait for assisting with operations involving the User struct
#[async_trait(?Send)]
pub trait UserHelper {
    /// Get Employee record from User info
    async fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error>;

    async fn save_user_settings(&mut self) -> Result<(), Error>;

    fn get_odoo_store_number(&mut self) -> Result<u64, Error>;

    fn get_store_from_odoo_id(&mut self) -> Result<Store, Error>;
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

/// A trait for assisting with operations involving Customer Records
#[async_trait(?Send)]
pub trait CustomerHelper {
    async fn find_associated_addr(&mut self) -> Result<prestashop_schema::Address, Error>;
}

#[async_trait(?Send)]
pub trait CustomerThreadHelper {
    async fn create_task_note_payload(&mut self) -> Result<(), Error>;
}

#[async_trait(?Send)]
pub trait CustomerDataHelper {
    async fn find_part_orders(&mut self) -> Result<Vec<SpecialPartOrder>, Error>;
    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
    async fn get_seb_data(&mut self) -> Result<ExtendedSeb, Error>;
}

#[async_trait(?Send)]
pub trait OrderHelper {
    async fn convert_to_task_payload(&mut self) -> Result<TaskPayload, Error>;
    async fn convert_to_ticket_payload(&mut self) -> Result<TicketPayload, Error>;
    async fn get_all_return_for_services(&mut self)
        -> Result<Vec<prestashop_schema::Order>, Error>;
}

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

#[async_trait(?Send)]
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
                ..Default::default()
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

#[async_trait(?Send)]
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
    async fn save_user_settings(&mut self) -> Result<(), Error> {
        let user_settings = serde_json::to_value(self.user_settings.clone())?;

        info!(
            "User Settings to apply: {user_settings:?}\nTo User: {:?}",
            self.id.clone()
        );

        match DATABASE
            .query("UPDATE user SET user_settings = $settings WHERE id == $user")
            .bind(("settings", user_settings))
            .bind(("user", self.id.clone()))
            .await
        {
            Ok(res) => {
                info!("Result: {res:?}");
            }
            Err(e) => info!("Error updating User Settings: {e:?}"),
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

/*
if let Some(usr) = self.current_user.clone(){
let email = usr.email.split_once('@').clone();
let username = email.unwrap_or_default().0.to_string();

let threads = self.messages.iter().map(|m| m.id_customer_thread.clone()).collect::<Vec<Option<String>>>();

let id_customer_thread = threads.get(0).cloned().unwrap_or_default();

let employee_id = usr.id_prestashop.clone().unwrap_or_default();
let id_employee = Some(employee_id.to_string());
let mut new_note = TaskNotePayload {
    everest_initials: usr.everest_initials,
    note: txt,
    task_id: self.task_id.clone(),
    username,
    user: Some(usr.id),
    id_employee,
    id_customer_thread,
    ..Default::default()
};

for thread in threads {
    if let Some(thread_id) = thread {
        new_note.id_customer_thread = Some(thread_id);
    }
}
info!("new_note: {new_note:?}");
spawn_local(async move {
    DATABASE
    .set("note", new_note)
    .await
    .unwrap();

    let update_task_note: Vec<Record> = DATABASE
    .query("CREATE task_note CONTENT $note")
    .await
    .unwrap()
    .take(0)
    .unwrap(); // CREATE task_note CONTENT $note
    info!("Update_note: {:?}", update_task_note);
});

 */
#[async_trait(?Send)]
pub trait TaskNotePayloadHelper {
    async fn check_tagged_user_in_note(&mut self) -> Result<(), anyhow::Error>;

    async fn create_task_note(&mut self) -> Result<(), anyhow::Error>;

    async fn create_prestashop_note(&mut self) -> Result<Value, anyhow::Error>;

    async fn update_task_note_in_db(
        &mut self,
        task_note: TaskNotePayload,
    ) -> Result<(), anyhow::Error>;

    async fn update_task_note_fields(&mut self) -> Result<(), anyhow::Error>;

    async fn update_task_note_with_current_time(&mut self) -> Result<(), anyhow::Error>;

    async fn update_username_if_needed(&mut self) -> Result<(), anyhow::Error>;

    async fn create_notification(
        &mut self,
        notification: Notification,
    ) -> Result<(), anyhow::Error>;

    async fn update_task_note_with_tagged_user(
        &mut self,
        user_id: RecordId,
    ) -> Result<(), anyhow::Error>;
}

#[async_trait(?Send)]
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
                // Create notification
                let notification = Notification {
                    notification_description: format!(
                        "tagged {} in task {}",
                        parse_email_user(&tagged_user.email),
                        id
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
        let _: Option<Record> = DATABASE
            .query("CREATE notification CONTENT $notif")
            .bind(("notif", notification))
            .await?
            .take(0)?;
            
        Ok(())
    }

    async fn update_task_note_with_tagged_user(&mut self, user_id: RecordId) -> Result<(), Error> {
        let _: Option<Record> = DATABASE
            .query("UPDATE task_note SET tagged_users += $user_id WHERE id == $id")
            .bind(("user_id", user_id))
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;
        Ok(())
    }

    async fn create_task_note(&mut self) -> Result<(), anyhow::Error> {
        self.check_tagged_user_in_note().await?;

        if self.id_customer_message.is_none()
            && self.id_customer_thread.is_some()
            && self.id_employee.is_some()
        {
            let response = self.create_prestashop_note().await?;
            if let (Some(date_add), Some(id)) = (response.get("date_add"), response.get("id")) {
                let date_str = date_add.to_string();
                let date = date_str.split_once(' ').unwrap_or_default().0;
                // Update task note with Prestashop details
                let updated_value = TaskNotePayload {
                    created_at: date.to_string(),
                    id: RecordId::from((TASK_NOTE_TABLE, id.to_string().clone())),
                    id_customer_message: Some(id.to_string().clone()),
                    ..self.clone() // Keep other fields the same
                };
                let diffs = self.diff(&updated_value);
                self.apply_mut(diffs);

                self.update_task_note_in_db(updated_value).await?;

                self.update_username_if_needed().await?;
            };

        } else if self.created_at.is_empty() {
            // Update created_at if missing
            self.update_task_note_with_current_time().await?;
        } else {
            // Handle other cases
            self.update_task_note_fields().await?;
        }

        Ok(())
    }

    async fn update_username_if_needed(&mut self) -> Result<(), Error> {
        // Logic to update username if needed
        Ok(())
    }

    async fn update_task_note_in_db(&mut self, task_note: TaskNotePayload) -> Result<(), Error> {
        let _: Option<Record> = DATABASE
            .query("UPDATE task_note MERGE $task_note")
            .bind(("task_note", task_note))
            .await?
            .take(0)?;
        Ok(())
    }

    async fn update_task_note_with_current_time(&mut self) -> Result<(), Error> {
        // Logic to update task note with the current time
        self.created_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    }

    async fn update_task_note_fields(&mut self) -> Result<(), Error> {
        // Logic to update task note fields
        Ok(())
    }

    async fn create_prestashop_note(&mut self) -> Result<Value, Error> {
        // Prepare the XML payload
        let begin = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><prestashop xmlns:xlink=\"http://www.w3.org/1999/xlink\">";
        let end = "</prestashop>";
        
        let id_employee = self.id_employee.as_deref().unwrap_or("");
        let id_customer_thread = self.id_customer_thread.as_deref().unwrap_or("");
        
        let payload = format!(
            "{}<customer_message><id_lang>1</id_lang><id_employee>{}</id_employee><id_customer_thread>{}</id_customer_thread><message>{}</message><private>1</private><id_order_message_type>0</id_order_message_type></customer_message>{}",
            begin, id_employee, id_customer_thread, self.note, end
        );

        // Send HTTP POST request with the XML payload
        let client = reqwest::Client::new();
        let response_text = client
            .post("https://pcl.master-tech.app/api/customer_messages")
            .header("Content-type", "application/xml")
            .body(payload)
            .send()
            .await?
            .text()
            .await?;

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
        Ok(
            serde_json::json!({
                date_add: date_add.to_string(),
                id: id.to_string(),
                date_upd: date_upd.to_string(),
            })
        ) 
    }
}

/// Parses the username from an email address
fn parse_email_user(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}


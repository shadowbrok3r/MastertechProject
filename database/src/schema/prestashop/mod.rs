use crate::schema::TASK_NOTE_TABLE;

use super::{deserializer::deserialize_to_string, helper_traits::EmployeeHelper, CustomerData, TaskNotePayload, User};
use chrono::{DateTime, Utc};
use log::info;
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE},
    Client,
};
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};
use std::collections::HashMap;
const PRESTASHOP_API_URL: &str = "https://pclaptops.mojo11.com/api";
const PRESTASHOP_API_URL_WASM: &str = "https://pcl.master-tech.app/api";

#[derive(Clone)]
pub struct Prestashop<'a> {
    client: Client,
    /// [field1,field2 …] or 'full'
    pub display: &'a str,
    /// &schema=synopsis for tests
    schema: Option<&'a str>,
    /**
     * [1|5]	    OR operator: list of possible values
     * [1,10]    Interval operator: define interval of possible values
     * [John]	Literal value (not case sensitive)
     * [Jo]%	    Begin operator: fields begins with the value (not case sensitive)
     * %[hn]	    End operator: fields ends with the value (not case sensitive)
     * %[oh]%	Contains operator: fields contains the value (not case sensitive)
     */
    filter: Option<&'a str>,
    /// number, or starting index (limit from number to the index)
    limit: Option<(i32, i32)>,
}

impl<'a> Default for Prestashop<'a> {
    fn default() -> Self {
        Self {
            client: Client::new(),
            schema: None,
            display: "full",
            filter: None,
            limit: None,
        }
    }
}

impl<'a> Prestashop<'a> {
    pub fn new<T: Deserialize<'a> + std::fmt::Debug + SubResource>(
        client: Client,
        display: &'a str,
        filter: Option<&'a str>,
        limit: Option<(i32, i32)>,
        schema: Option<&'a str>,
    ) -> Self {
        Self {
            client,
            display,
            filter,
            limit,
            schema,
        }
    }

    pub fn query_args(&self, resource_name: &str, url_params: HashMap<&str, &str>) -> String {
        let base_url = format!("{PRESTASHOP_API_URL}/{resource_name}");

        let mut query_params = vec![];

        // Adding `display` parameter
        if !self.display.is_empty() {
            query_params.push(format!("display={}", self.display));
        }

        // Adding `schema` parameter if present
        if let Some(ref schema) = self.schema {
            query_params.push(format!("schema={}", schema));
        }

        // Adding `filter` parameter if present
        if let Some(ref filter) = self.filter {
            query_params.push(format!("filter[{}]={}", resource_name, filter));
        }

        // Adding `limit` parameter if present
        if let Some((start, end)) = self.limit {
            query_params.push(format!("limit={},{}", start, end));
        }

        // Adding other URL parameters
        for (key, value) in url_params {
            query_params.push(format!("{}={}", key, value));
        }

        // Constructing the final URL
        let query_string = if !query_params.is_empty() {
            format!("?{}", query_params.join("&"))
        } else {
            String::new()
        };

        format!("{}{}", base_url, query_string)
    }

    pub fn query_args_wasm(&self, resource_name: &str, url_params: HashMap<&str, &str>) -> String {
        let base_url = format!("{PRESTASHOP_API_URL_WASM}/{resource_name}");

        let mut query_params = vec![];

        // Adding `display` parameter
        if !self.display.is_empty() {
            query_params.push(format!("display={}", self.display));
        }

        // Adding `schema` parameter if present
        if let Some(ref schema) = self.schema {
            query_params.push(format!("schema={}", schema));
        }

        // Adding `filter` parameter if present
        if let Some(ref filter) = self.filter {
            query_params.push(format!("filter[{}]={}", resource_name, filter));
        }

        // Adding `limit` parameter if present
        if let Some((start, end)) = self.limit {
            query_params.push(format!("limit={},{}", start, end));
        }

        // Adding other URL parameters
        for (key, value) in url_params {
            query_params.push(format!("{}={}", key, value));
        }

        // Constructing the final URL
        let query_string = if !query_params.is_empty() {
            format!("?{}", query_params.join("&"))
        } else {
            String::new()
        };

        format!("{}{}", base_url, query_string)
    }

    pub async fn request_subresources_by_id<T>(
        &self,
        resource: &str,
        name: &str,
        id: &str,
    ) -> anyhow::Result<T, anyhow::Error>
    where
        T: for<'de> Deserialize<'de> + std::fmt::Debug,
    {
        let url = format!("{PRESTASHOP_API_URL}/{resource}/{id}?output_format=JSON");
        let response: Value = self
            .client
            .get(url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .send()
            .await?
            .json()
            .await?;

        info!("prestashop_schema -> query:{url}\nresponse: {:#?}", response);

        let x: T = from_value(response[name].clone())?;
        // info!("prestashop_schema -> x: {x:#?}");
        Ok(x)
    }

    pub async fn request_subresources_by_id_wasm<T>(
        &self,
        resource: &str,
        name: &str,
        id: &str,
    ) -> anyhow::Result<T, anyhow::Error>
    where
        T: for<'de> Deserialize<'de> + std::fmt::Debug + Send,
    {
        let url = if !self.display.is_empty() && self.display.ne("full"){
            format!("{PRESTASHOP_API_URL_WASM}/{resource}/{id}?display={}&output_format=JSON", self.display)
        } else {
            format!("{PRESTASHOP_API_URL_WASM}/{resource}/{id}?output_format=JSON")
        };

        let response: Value = self
            .client
            .get(url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .send()
            .await?
            .json()
            .await?;

        info!("prestashop_schema -> query:{url}");

        let x: T = from_value(response[name].clone())?;
        // info!("prestashop_schema -> x: {x:#?}");
        Ok(x)
    }

    pub async fn request_resources<T>(
        &self,
        resource_name: &str,
        url_params: HashMap<&str, &str>,
    ) -> anyhow::Result<Vec<T>, anyhow::Error>
    where
        T: for<'de> Deserialize<'de> + std::fmt::Debug,
    {
        info!(
            "resource_name: {resource_name:#?}, {url_params:#?}\nURL: {:#?}",
            self.query_args(resource_name, url_params.clone())
        );

        let response: Value = self
            .client
            .get(self.query_args(resource_name, url_params))
            .send()
            .await?
            .json()
            .await?;

        info!("prestashop_schema -> response: {:#?}", response);
        let x: Vec<T> = from_value(response[resource_name].clone())?;
        info!("prestashop_schema -> x: {x:#?}");

        Ok(x)
    }

    pub async fn request_resources_wasm<T>(
        &self,
        resource_name: &str,
        url_params: HashMap<&str, &str>,
    ) -> anyhow::Result<Vec<T>, anyhow::Error>
    where
        T: for<'de> Deserialize<'de> + std::fmt::Debug + Send + Default,
    {
        log::info!(
            "resource_name: {resource_name:#?}, {url_params:#?}\nURL: {:#?}",
            self.query_args_wasm(resource_name, url_params.clone())
        );

        let response: Value = self
            .client
            .get(self.query_args_wasm(resource_name, url_params))
            .send()
            .await?
            .json()
            .await?;

        // info!("prestashop_schema -> response: {:#?}", response);
        let x: anyhow::Result<Vec<T>, serde_json::Error> = from_value(response[resource_name].clone());
        // info!("prestashop_schema -> x: {x:#?}");
        if let Err(e) = x {
            log::info!("Error: {e:?}");
            let mut new = Vec::new();
            new.push(T::default());
            return Ok(new)
        }

        Ok(x?)
    }

    pub async fn find_resource_wasm<T>(
        &self,
        resource_name: &str,
        url_params: HashMap<&str, &str>,
    ) -> anyhow::Result<T, anyhow::Error>
    where
        T: for<'de> Deserialize<'de> + std::fmt::Debug + Send,
    {
        info!(
            "resource_name: {resource_name:#?}, {url_params:#?}\nURL: {:#?}",
            self.query_args_wasm(resource_name, url_params.clone())
        );

        let response: Value = self
            .client
            .get(self.query_args_wasm(resource_name, url_params))
            .send()
            .await?
            .json()
            .await?;

        info!("prestashop_schema -> response: {:#?}", response);
        let t: T = from_value(response[resource_name].get(0).cloned().unwrap_or_default())?;
        info!("prestashop_schema -> Value: {t:?}");
        // let x: T = from_value(t.get(0).cloned().unwrap_or_default())?;
        // info!("prestashop_schema -> x: {x:#?}");

        Ok(t)
    }

    pub async fn delete_resource_wasm(
        &self,
        resource_name: &str,
        id: &str,
    ) 
        -> anyhow::Result<String, anyhow::Error>
    {
        let base_url = format!("{PRESTASHOP_API_URL_WASM}/{resource_name}/{id}");
        info!("prestashop_schema -> URL: {base_url:?}");

        let response: String = self.client.delete(base_url).send().await?.text().await?;

        info!("prestashop_schema -> response: {:#?}", response);

        Ok(response) 
    }

    pub async fn create_customer_thread(
        &self, 
        service_number: &str, 
        id_customer: &str
    ) 
        -> anyhow::Result<super::helper_traits::PrestaResourceResponse, anyhow::Error> 
    {
        // Prepare the XML payload
        let payload = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><prestashop xmlns:xlink="http://www.w3.org/1999/xlink">
                <customer_thread>
                    <id_lang>1</id_lang>
                    <id_contact>0</id_contact>
                    <id_order>{}</id_order>
                    <token>{}</token>
                    <id_customer >{}</id_customer>
                </customer_thread>
            </prestashop>"#, 
            service_number, service_number, id_customer
        );

        // Send HTTP POST request with the XML payload
        info!("prestashop_schema -> Payload: {:?}", payload);
        let response_text = self.client
            .post(format!("{PRESTASHOP_API_URL_WASM}/customer_threads"))
            .header("Content-type", "application/xml")
            .body(payload)
            .send()
            .await?
            .text()
            .await?;

        info!("prestashop_schema -> response text: {response_text:?}");
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

        Ok(super::helper_traits::PrestaResourceResponse {
            date_add: super::helper_traits::convert_date_string(date_add)?.to_string(), //,
            id: id.to_string(),
            date_upd: super::helper_traits::convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        })
    }

    pub async fn create_customer_message(
        &self,
        id_employee: &str,
        id_customer_thread: &str,
        note: &str
    ) -> anyhow::Result<super::helper_traits::PrestaResourceResponse, anyhow::Error> {
        // Prepare the XML payload
        let payload = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><prestashop xmlns:xlink="http://www.w3.org/1999/xlink">
                <customer_message>
                    <id_lang>1</id_lang>
                    <id_employee>{}</id_employee>
                    <id_customer_thread>{}</id_customer_thread>
                    <message>{}</message>
                    <private>1</private>
                    <id_order_message_type>0</id_order_message_type>
                </customer_message>
            </prestashop>"#,
            id_employee, id_customer_thread, note
        );

        // Send HTTP POST request with the XML payload
        info!("prestashop_schema -> Payload: {:?}", payload);
        let response_text = self.client
            .post(format!("{PRESTASHOP_API_URL_WASM}/customer_messages"))
            .header("Content-type", "application/xml")
            .body(payload)
            .send()
            .await?
            .text()
            .await?;

        info!("prestashop_schema -> response text: {response_text:?}");
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

        Ok(super::helper_traits::PrestaResourceResponse {
            date_add: super::helper_traits::convert_date_string(date_add)?.to_string(), //,
            id: id.to_string(),
            date_upd: super::helper_traits::convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        })
    }

    pub async fn modify_customer_message(
        &self,
        id_customer_message: &str,
        id_employee: &str,
        id_customer_thread: &str,
        note: &str
    ) -> anyhow::Result<super::helper_traits::PrestaResourceResponse, anyhow::Error> {
        // Prepare the XML payload
        let payload = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><prestashop xmlns:xlink="http://www.w3.org/1999/xlink">
                <customer_message>
                    <id_lang>1</id_lang>
                    <id_employee>{id_employee}</id_employee>
                    <id_customer_thread>{id_customer_thread}</id_customer_thread>
                    <id>{id_customer_message}</id>
                    <message>{note}</message>
                    <private>1</private>
                    <id_order_message_type>0</id_order_message_type>
                </customer_message>
            </prestashop>"#
        );

        // Send HTTP POST request with the XML payload
        info!("prestashop_schema -> Payload: {:?}", payload);
        let response_text = self.client
            .put(format!("{PRESTASHOP_API_URL_WASM}/customer_messages"))
            .header("Content-type", "application/xml")
            .body(payload)
            .send()
            .await?
            .text()
            .await?;

        info!("prestashop_schema -> response text: {response_text:?}");
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

        Ok(super::helper_traits::PrestaResourceResponse {
            date_add: super::helper_traits::convert_date_string(date_add)?.to_string(), //,
            id: id.to_string(),
            date_upd: super::helper_traits::convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        })
    }
}

impl CustomerMessage {
    pub async fn into_task_note(&self, service_number: &str) -> anyhow::Result<TaskNotePayload, anyhow::Error> {
        match Employee::default().get_employee_from_id(&self.id_employee).await {
            Ok(employee) => {
                match User::query_user_from_email(employee.email.clone()).await {
                    Ok(user) => { 
                        log::warn!("Pulled user: {}", user.get_name());
                        return Ok(TaskNotePayload {
                            note: self.message.clone(),
                            created_at: DateTime::parse_from_rfc3339(&self.date_add)?.with_timezone(&Utc).into(),
                            id: surrealdb::RecordId::from((TASK_NOTE_TABLE, self.id.clone())),
                            username: user.get_username().to_string(),
                            user: Some(user.get_id()),
                            id_customer_thread: Some(self.id_customer_thread.clone()),
                            id_customer_message: Some(self.id.clone()),
                            id_employee: Some(self.id_employee.clone()),
                            service_number: Some(service_number.to_string()),
                            ..Default::default()
                        });
                    },
                    Err(e) => Err(anyhow::anyhow!("Error querying user from email: {e:?}")),
                }
            },
            Err(e) => Err(anyhow::anyhow!("Error converting customer message into task note: {e:?}")),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct PrestashopPayload {
    pub customer: CustomerData,
    pub order: Order,
    pub sales_rep: Option<Employee>,
    pub split_rep: Option<Employee>,
    pub address: Address,
    pub customer_threads: Vec<CustomerThread>,
    pub customer_messages: Vec<CustomerMessage>,
    pub task_notes: Vec<TaskNotePayload>
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Address {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_customer: String,  // ❌     isNullOrUnsignedId
    pub lastname: String,     // ✔️     isName
    pub firstname: String,    // ✔️     isName
    pub address1: String,     // ✔️     isAddress
    pub address2: String,     // ❌     isAddress
    pub postcode: String,     // ❌     isPostCode
    pub city: String,         // ✔️     isCityName
    pub phone: String,        // ❌     isPhoneNumber
    pub phone_mobile: String, // ❌     isPhoneNumber
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Employee {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_store: String,
    pub lastname: String,
    pub firstname: String,
    pub email: String,
    pub initials: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Order {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type_name: Option<String>,
    #[serde(default)]
    pub id_address_delivery: String, // ✔️
    #[serde(default)]
    pub id_address_invoice: String,  // ✔️
    #[serde(default)]
    pub id_customer: String,         // ✔️
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub invoice_number: String,
    #[serde(default)]
    pub invoice_date: String,  
    #[serde(default)]
    pub payment: String,
    #[serde(default)]
    pub date_add: String,
    #[serde(default)]
    pub date_upd: String,
    #[serde(default)]
    pub id_employee_sales_rep: String,
    #[serde(default)]
    pub id_employee_split_rep: String,
    #[serde(default)]
    pub id_employee_editing: String,
    #[serde(default)]
    pub id_order_everest: String,
    #[serde(default)]
    pub id_store: String,   // 1 = warehouse
    #[serde(default)]
    pub total_paid: String, // ✔️
    #[serde(default)]
    pub total_products_wt: String,
    #[serde(default)]
    pub reference: String, // what prestashop sees since order id and reference are different...
    #[serde(default)]
    pub id_order_parent: String, // no idea
    #[serde(default)]
    pub shipping_number: String, // Tracking number
    #[serde(default)]
    pub order_type: String, // Configurator / Sales Order
    // note: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub associations: Associations,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Associations {
    #[serde(default = "new_vec")]
    pub order_rows: Vec<OrderRow>,
    #[serde(default = "new_svc_vec")]
    pub order_service: Vec<ServiceOrder>,
}

fn new_vec() -> Vec<OrderRow> {
    Vec::new()
}

fn new_svc_vec() -> Vec<ServiceOrder> {
    Vec::new()
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct OrderRow {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_order_config: String,
    pub product_id: String,
    pub product_quantity: String,
    pub product_name: String,
    pub product_price: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Customer {
    pub lastname: String,
    pub firstname: String,
    pub email: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustomerMessage {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_employee: String,
    pub id_customer_thread: String,
    pub message: String,
    pub file_name: String,
    pub private: String,
    pub date_add: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustomerThread {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_customer: String, // isUnsignedId 	❌ 		Customer ID
    pub id_order: String,    // isUnsignedId 	❌ 		Order ID
    pub date_add: String,    // isDate 	        ❌
    pub date_upd: String,    // isDate 	        ❌
    pub associations: CustMessageAssociation,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustMessageAssociation {
    pub customer_messages: Vec<CustMessage>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustMessage {
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct OrderDetails {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,

    pub id_order: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ServiceOrder {
    // pub id: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id_order_service: String,
    // pub id_order: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_name: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_mfg: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_model: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_serial: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_password: String,
    // pub id_status_service: String, // This is fucky
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_power_supply: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub other_hardware_software: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub physical_damage: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub check_in_notes: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub intake_notes: String,
    // pub id_employee_qc_tech: String,
    // pub id_employee_qc_signoff: String,
}

#[derive(Serialize, Debug, Default, PartialEq)]
pub struct Resources {
    /// 	The Customer, Manufacturer and Customer addresses
    pub addresses: Address,
    /// 	The product Attachments
    pub attachments: String,
    /// 	The product Attachments files
    pub attachments_file: String,
    /// 	Customer’s carts
    pub carts: String,
    /// 	Customer services messages
    pub customer_messages: String,
    /// 	Customer services threads
    pub customer_threads: String,
    /// 	The e-shop’s customers
    pub customers: String,
    /// 	The Employees
    pub employees: Employee,
    /// 	The customers messages
    pub messages: String,
    /// 	Details of an order
    pub order_details: String,
    /// 	The Order histories
    pub order_histories: String,
    /// 	The Order invoices
    pub order_invoices: String,
    /// 	The Customers orders
    pub orders: String,
    /// 	The products
    pub products: String,
    /// 	Search
    pub search: String,
    /// 	Available quantities of products
    pub stock_availables: String,
    /// 	Stocks for products
    pub stocks: String,
    /// 	The stores
    pub stores: String,
}

pub trait SubResource {
    fn get_subresource(&self, field: &str) -> Option<String>;
    fn get_name(&self) -> String;
    fn get_resource_name(&self) -> String;
}

impl SubResource for Employee {
    fn get_subresource(&self, field: &str) -> Option<String> {
        match field {
            "id" => Some(self.id.to_string()),
            "lastname" => Some(self.lastname.clone()),
            "firstname" => Some(self.firstname.clone()),
            "email" => Some(self.email.clone()),
            _ => None,
        }
    }

    fn get_resource_name(&self) -> String {
        "employees".to_string()
    }

    fn get_name(&self) -> String {
        "employee".to_string()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MissedCallOrder {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    // 2025-04-04 16:48:01
    pub date_add: String,
    #[serde(default, skip_deserializing)]
    pub missing_days: Vec<String>,
}

#[derive(Default, Deserialize, Serialize, PartialEq)]
pub enum PrestashopOrderType {
    #[default]
    CheckinShelf,
    InRepair,
    DoneShelf
}

impl PrestashopOrderType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::CheckinShelf => "checkinShelf",
            Self::InRepair => "inRepair",
            Self::DoneShelf => "doneShelf",
        }
    }

    // 30=In Repair, 239=Accepted by Odoo?, 29=CheckinShelf, 40=DoneShelf, 73=Order Placed, 70=PrePulled236=ShipToStore
    pub fn id(&self) -> &str {
        match self {
            Self::CheckinShelf => "29",
            Self::InRepair => "30",
            Self::DoneShelf => "40",
        }
    }

    pub const VALUES: [Self; 3] = [
        Self::CheckinShelf,
        Self::InRepair,
        Self::DoneShelf,
    ];
}
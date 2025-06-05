use super::{deserializer::deserialize_to_string, CustomerData, TaskNotePayload};
use reqwest::{ header::{ACCEPT, CONTENT_TYPE}, Client};
use serde_json::{from_value, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use log::info;

const PRESTASHOP_API_URL: &str = "https://pclaptops.mojo11.com/api";
const PRESTASHOP_API_URL_WASM: &str = "https://pcl.master-tech.app/api";

pub mod customer_messages;
pub mod customer_threads;
pub mod koth;

pub use customer_messages::*;
pub use customer_threads::*;
pub use koth::*;

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

        log::info!("prestashop_schema -> query:{url}\nResponse: {:?}", response[name].clone());

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
            "resource_name: {resource_name}, {url_params:#?}\nURL: {}",
            self.query_args_wasm(resource_name, url_params.clone())
        );

        let response: Value = self
            .client
            .get(self.query_args_wasm(resource_name, url_params))
            .send()
            .await?
            .json()
            .await?;

        log::info!(
            "Raw response: {response:?}",
        );
        match from_value::<Vec<T>>(response[resource_name].clone()) {
            Ok(t) => Ok(t),
            Err(e) => {
                log::error!("request_resources_wasm<T: {resource_name}> -> Error: {e:?}");
                return Ok(vec![T::default()]);
            }
        }
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
    // #[serde(skip_serializing_if = "Option::is_none")]
    // #[serde(deserialize_with = "deserialize_to_string")]
    pub id_order_type: String,
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
    pub delivery_date: String,
    #[serde(default)]
    pub total_products_wt: String,
    #[serde(default)]
    pub total_paid_tax_excl: String,
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

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub enum OrderState {
    #[default]
    AcceptedByOdoo,
    Shipped,
    DeliveredToStore,
    DoneShelf,
    OrderPlaced,
    PrePulled,
    ReadyToBuild,
    QcAndBurnin,
    ShipToStore,
    Returned 
}

impl OrderState {
    pub fn as_str(&self) -> &str {
        match self {
            Self::AcceptedByOdoo => "Accepted By Odoo",
            Self::Shipped => "Shipped",
            Self::DeliveredToStore => "Delivered To Store",
            Self::DoneShelf => "Done Shelf",
            Self::OrderPlaced => "Order Placed",
            Self::PrePulled => "Pre Pulled",
            Self::ReadyToBuild => "Ready To Build",
            Self::QcAndBurnin => "Qc & Burnin",
            Self::ShipToStore => "Ship To Store",
            Self::Returned => "Returned",
        }
    }

    pub fn from_id_str(id: &str) -> &str {
        match id {
            "239" => "Accepted By Odoo",
            "4" => "Shipped",
            "238" => "Delivered To Store",
            "40" => "Done Shelf",
            "73" => "Order Placed",
            "70" => "Pre Pulled",
            "224" => "Ready To Build",
            "71" => "Qc & Burnin",
            "236" => "Ship To Store",
            "84" => "Returned",
            "30" => "In Repair",
            "29" => "Check-in Shelf",
            _ => "Accepted By Odoo"
        }
    }

    pub fn state_from_id_str(id: &str) -> Self {
        match id {
            "239" => Self::AcceptedByOdoo,
            "4" => Self::Shipped,
            "238" => Self::DeliveredToStore,
            "40" => Self::DoneShelf,
            "73" => Self::OrderPlaced,
            "70" => Self::PrePulled,
            "224" => Self::ReadyToBuild,
            "71" => Self::QcAndBurnin,
            "236" => Self::ShipToStore,
            "84" => Self::Returned,
            _ => Self::AcceptedByOdoo
        }
    }

    /*84=Returned, 30=In Repair, 239=Accepted by Odoo?, 29=CheckinShelf, 40=DoneShelf, 73=Order Placed, 70=PrePulled236=ShipToStore */
    pub fn id(&self) -> i32 {
        match self {
            Self::AcceptedByOdoo => 239,
            Self::Shipped => 4,
            Self::DeliveredToStore => 238,
            Self::DoneShelf => 40,
            Self::OrderPlaced => 73,
            Self::PrePulled => 70,
            Self::ReadyToBuild => 224,
            Self::QcAndBurnin => 71,
            Self::ShipToStore => 236,
            Self::Returned => 84,
        }
    }

    pub const VALUES: [Self; 10] = [
        Self::AcceptedByOdoo,
        Self::Shipped,
        Self::DeliveredToStore,
        Self::DoneShelf,
        Self::OrderPlaced,
        Self::PrePulled,
        Self::ReadyToBuild,
        Self::QcAndBurnin,
        Self::ShipToStore,
        Self::Returned,
    ];
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
    pub product_reference: String
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct OrderPayment {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub order_reference: String,
    pub id_currency: String,
    pub amount: String,
    pub payment_method: String,
    pub conversion_rate: String,
    pub transaction_id: String,
    pub card_number: String,
    pub card_brand: String,
    pub card_expiration: String,
    pub card_holder: String,
    pub date_add: String,
    pub payment_period: String,
    pub check_finance_num: String,
    pub chargeafter_response: String,
    pub capture_number: String,
    pub id_order: String,
    pub id_module: String,
    pub id_odoo_payment: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Customer {
    pub lastname: String,
    pub firstname: String,
    pub email: String,
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
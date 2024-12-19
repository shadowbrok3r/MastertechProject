use super::{deserializer::deserialize_to_string, CustomerData};
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

        info!("query:{url}\nresponse: {:#?}", response);

        let x: T = from_value(response[name].clone())?;
        // info!("x: {x:#?}");
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

        info!("query:{url}");

        let x: T = from_value(response[name].clone())?;
        // info!("x: {x:#?}");
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

        info!("response: {:#?}", response);
        let x: Vec<T> = from_value(response[resource_name].clone())?;
        info!("x: {x:#?}");

        Ok(x)
    }

    pub async fn request_resources_wasm<T>(
        &self,
        resource_name: &str,
        url_params: HashMap<&str, &str>,
    ) -> anyhow::Result<Vec<T>, anyhow::Error>
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

        // info!("response: {:#?}", response);
        let x: Vec<T> = from_value(response[resource_name].clone())?;
        // info!("x: {x:#?}");

        Ok(x)
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

        info!("response: {:#?}", response);
        let t: T = from_value(response[resource_name].get(0).cloned().unwrap_or_default())?;
        info!("Value: {t:?}");
        // let x: T = from_value(t.get(0).cloned().unwrap_or_default())?;
        // info!("x: {x:#?}");

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
        info!("URL: {base_url:?}");

        let response: String = self.client.delete(base_url).send().await?.text().await?;

        info!("response: {:#?}", response);

        Ok(response) 
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct PrestashopPayload {
    pub customer: CustomerData,
    pub order: Order,
    pub sales_rep: Option<Employee>,
    pub split_rep: Option<Employee>,
    pub address: Address,
    pub customer_threads: Vec<CustomerThread>,
    pub customer_messages: Vec<CustomerMessage>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Employee {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_store: String,
    pub lastname: String,
    pub firstname: String,
    pub email: String,
    pub initials: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct OrderRow {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_order_config: String,
    pub product_id: String,
    pub product_quantity: String,
    pub product_name: String,
    pub product_price: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Customer {
    pub lastname: String,
    pub firstname: String,
    pub email: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct CustomerThread {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_customer: String, // isUnsignedId 	❌ 		Customer ID
    pub id_order: String,    // isUnsignedId 	❌ 		Order ID
    pub date_add: String,    // isDate 	        ❌
    pub date_upd: String,    // isDate 	        ❌
    pub associations: CustMessageAssociation,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct CustMessageAssociation {
    pub customer_messages: Vec<CustMessage>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct CustMessage {
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct OrderDetails {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,

    pub id_order: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
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

#[derive(Serialize, Debug, Default)]
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

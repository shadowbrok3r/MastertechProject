use crate::app_state::MastertechContext;
use database::schema::{
    prestashop_schema::{
        Address, Customer, CustomerMessage, CustomerThread, Employee, Order, PrestashopPayload,
        SubResource,
    },
    CustomerData, CustomerId, CUSTOMER_TABLE,
};
use log::{error, info};
use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use serde::Deserialize;
use serde_json::{from_value, Value};
use std::collections::HashMap;
use surrealdb::sql::Thing;

const AUTH_TOKEN: &str = "Basic SVAxUlE2UkZSTUZXQjZCOFdIUVY4RFpQV1ZOTDIxWE06";

impl MastertechContext {
    pub fn presta_api(&mut self) {
        let input = self.ticket_data.service_number.clone();
        let tx = self.prestashop_api_tx.clone();
        if !input.is_empty() {
            tokio::spawn(async move {
                let api_call = Prestashop::default();
                let mut query = HashMap::new();

                query.insert("filter[id_order]", input.as_str());
                query.insert("output_format", "JSON");

                let customer_threads: Vec<CustomerThread> = api_call
                    .request_resources("customer_threads", query.clone())
                    .await
                    .unwrap_or_default();

                let mut customer_messages: Vec<CustomerMessage> = Vec::new();

                if !customer_threads.is_empty() {
                    for thread in customer_threads.iter() {
                        for msg in thread.associations.customer_messages.iter() {
                            match api_call
                                .request_subresources_by_id(
                                    "customer_messages",
                                    "customer_message",
                                    msg.id.as_str(),
                                )
                                .await
                            {
                                Ok(msg) => customer_messages.push(msg),
                                Err(e) => info!("Error getting customer messages: {e:?}"),
                            }
                        }
                    }
                }

                let order: Order = api_call
                    .request_subresources_by_id("orders", "order", &input)
                    .await
                    .unwrap();

                if order.id_customer.is_empty() {
                    info!("Order is likely gonna fuKKKK");
                }

                info!("order: {order:#?}");

                let sales_rep: Option<Employee> = if !order.id_employee_sales_rep.contains("0") {
                    //|| order.id_employee_sales_rep.len() != 0{
                    let employee: Employee = api_call
                        .request_subresources_by_id(
                            "employees",
                            "employee",
                            &order.id_employee_sales_rep,
                        )
                        .await
                        .unwrap();

                    info!("employee: {employee:#?}");
                    Some(employee)
                } else {
                    None
                };

                let split_rep: Option<Employee> = if !order.id_employee_split_rep.contains("0") {
                    let employee_2: Employee = api_call
                        .request_subresources_by_id(
                            "employees",
                            "employee",
                            &order.id_employee_split_rep,
                        )
                        .await
                        .unwrap();

                    info!("employee: {sales_rep:#?}");
                    Some(employee_2)
                } else {
                    None
                };

                let cust: Customer = api_call
                    .request_subresources_by_id("customers", "customer", &order.id_customer)
                    .await
                    .unwrap();

                // info!("customer: {customer:#?}");

                let address: Address = api_call
                    .request_subresources_by_id("addresses", "address", &order.id_address_invoice)
                    .await
                    .unwrap();

                // let notes: CustomerThread = api_call.request_subresources_by_id(
                //     "customer_threads",
                //     "customer_thread",
                //     &order.id_address_delivery
                // ).await.unwrap();

                info!("address: {address:#?}");

                let customer = CustomerData {
                    id: Some(CustomerId(Thing::from((
                        CUSTOMER_TABLE.to_string(),
                        order.id_customer.clone(),
                    )))),
                    cust_code: order.id_customer.clone(),
                    name: format!("{} {}", &cust.firstname, &cust.lastname),
                    phone_number: address.phone.clone().to_string(),
                    // phone_number_2: address.phone_mobile.clone().unwrap_or(0).to_string(),
                    email: cust.email,
                    ..Default::default()
                };

                let presta_payload = PrestashopPayload {
                    customer,
                    order,
                    sales_rep,
                    split_rep,
                    address,
                    customer_threads,
                    customer_messages,
                };

                match tx.try_send(presta_payload) {
                    Ok(_) => drop(tx),
                    Err(err) => error!("Error: {err:?}"),
                };
            });
        }
    }
}

pub struct Prestashop<'a> {
    client: Client,
    /// [field1,field2 …] or 'full'
    display: &'a str,
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
    // data_channel: PrestaDataChannel
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
        let base_url = format!("https://pclaptops.mojo11.com/api/{}", resource_name);

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
        let url = format!("https://pclaptops.mojo11.com/api/{resource}/{id}?output_format=JSON");
        let response: Value = self
            .client
            .get(url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .json()
            .await?;

        info!("query:{url}\nresponse: {:#?}", response);

        let x: T = from_value(response[name].clone())?;
        info!("x: {x:#?}");
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
            .header(AUTHORIZATION, AUTH_TOKEN)
            .send()
            .await?
            .json()
            .await?;

        info!("response: {:#?}", response);
        let x: Vec<T> = from_value(response[resource_name].clone())?;
        info!("x: {x:#?}");

        Ok(x)
    }
}

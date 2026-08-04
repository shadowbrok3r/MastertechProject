use std::collections::HashMap;

use structdiff::{Difference, StructDiff};
use crate::{schema::{prestashop::{Address, Customer, Prestashop}, CUSTOMER_TABLE}, db};

use super::{random_record_id, RecordId, SurrealValue};


#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference, SurrealValue)]
pub struct CustomerData {
    pub id: RecordId,
    pub cust_code: String,
    pub part_order_links: Option<Vec<String>>,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String,
    pub email: String,
    pub li_doc: String,
    pub li_amnt: String,
    pub num_inv: String
}

impl Default for CustomerData {
    fn default() -> Self {
        Self {
            id: random_record_id(CUSTOMER_TABLE),
            cust_code: Default::default(),
            part_order_links: Default::default(),
            name: Default::default(),
            phone_number: Default::default(),
            phone_number_2: Default::default(),
            email: Default::default(),
            li_doc: Default::default(),
            li_amnt: Default::default(),
            num_inv: Default::default(),
        }
    }
}

impl CustomerData {
    pub async fn get_associated_customer(id: RecordId) -> anyhow::Result<Self, anyhow::Error> {
        let customer: Option<Self> = db()
            .query("SELECT VALUE service_ticket.customer.* FROM task WHERE id == $id")
            .bind(("id", id))
            .await?
            .take(0)?;
        Ok(customer.unwrap_or_default())
    }

    pub async fn get_customers(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let customers: Vec<Self> = db()
            .query("SELECT * FROM customer START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(customers)
    }

    pub async fn find_customer_by_id(id_customer: &str) -> anyhow::Result<Self, anyhow::Error> {
        let api_call = Prestashop::default();
        let mut query = HashMap::new();
        let mut tmp_address = Address::default();
        query.insert("filter[id_customer]", id_customer);
        query.insert("output_format", "JSON");

        let addresses: Vec<Address> = api_call
            .request_resources_checked("addresses", query.clone())
            .await?;

        if let Some(address) = addresses.get(0) {
            tmp_address = address.clone();
        }

        let cust: Customer = api_call
            .request_subresources_by_id_wasm("customers", "customer", id_customer)
            .await?;

        Ok(CustomerData { 
            id: RecordId::new(
                CUSTOMER_TABLE,
                id_customer,
            ),
            cust_code: id_customer.to_string(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            phone_number: tmp_address.phone.clone().to_string(),
            email: cust.email,
            phone_number_2: tmp_address.phone_mobile.clone().to_string(),
            ..Default::default()
        })
    }
}

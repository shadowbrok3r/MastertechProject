use crate::{
    DATABASE, schema::{LiveTaskPayload, TaskPayload, TicketPayload, helper_traits::PrestashopPayloadHelper, prestashop_schema::{CustomerMessage, CustomerThread, MissedCallOrder, Prestashop}, utilities::get_missing_call_days}
};
use log::debug;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};
use surrealdb::RecordId;

use super::utilities::Task;

use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use crossbeam::channel::Sender;
use surrealdb::Action;

pub struct NewTicketChannel {
    pub new_ticket: TicketPayload,
    pub new_task: (Action, LiveTaskPayload),
}


pub async fn get_associated_ticket(
    tx: Sender<NewTicketChannel>,
    new_task: (Action, LiveTaskPayload),
) -> Result<(), Error> {
    debug!("get_associated_ticket");
    let service_num = new_task.1.clone().service_number.unwrap_or_default();
    DATABASE.set("service_num", service_num).await?;
    let ticket: Option<TicketPayload> = DATABASE.query(format!("SELECT * FROM service_order WHERE service_number == $service_num FETCH computer, customer")).await?.take(0)?;
    debug!("ticket: {:?}", ticket);
    let new_ticket = ticket.unwrap_or_default();
    let chnnl = NewTicketChannel {
        new_ticket,
        new_task,
    };
    tx.try_send(chnnl)?;
    Ok(())
}

pub async fn get_services_by_status(
    status: &str, 
    store: &str
) -> anyhow::Result<Vec<MissedCallOrder>, anyhow::Error> {
    let mut api_call = Prestashop::default();
    let mut query: HashMap<&str, &str> = HashMap::new();
    let mut missed_orders = Vec::new();
    
    query.insert("filter[current_state]", status);
    query.insert("filter[id_order_type]", "2");
    query.insert("filter[id_store]", store);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");
    api_call.display = "[id, date_add]";

    let orders: Vec<MissedCallOrder> = api_call
        .request_resources_wasm("orders", query.clone())
        .await
        .context("Pulling orders list")?;

    log::info!("Orders: {orders:?}");
    log::info!("Api query: {query:?}");

    for order in orders.iter() {
        let api_call = Prestashop::default();
        let mut query = HashMap::new();
        
        if order.id.is_empty() {
            break;
        }

        log::info!("Pulling order {}", order.id);
        
        query.insert("filter[id_order]", order.id.as_str());
        query.insert("output_format", "JSON");

        let customer_threads: Vec<CustomerThread> = api_call
            .request_resources_wasm("customer_threads", query.clone())
            .await?;

        let mut customer_messages: Vec<CustomerMessage> = Vec::new();

        if !customer_threads.is_empty() {
            for thread in customer_threads.iter() {
                for msg in thread.associations.customer_messages.iter() {
                    let msg = api_call
                        .request_subresources_by_id_wasm(
                            "customer_messages",
                            "customer_message",
                            msg.id.as_str(),
                        )
                        .await?;
                    customer_messages.push(msg);
                }
            }
        }
        
        // Get the missing days for this order.
        let missing_days = get_missing_call_days(&order.date_add, &customer_messages);
        
        // Only include orders with missing call days.
        if !missing_days.is_empty() {
            missed_orders.push(MissedCallOrder {
                date_add: order.date_add.clone(),
                id: order.id.clone(),
                missing_days,
            });
        }
    }

    log::info!("Missed orders: {:#?}", missed_orders);

    Ok(missed_orders)
}

pub async fn get_order_info_from_serial(serial: &str) -> anyhow::Result<crate::schema::prestashop::PrestashopPayload, anyhow::Error> {
    match get_order_from_prestashop_payload(serial).await {
        Ok(order) => Ok(order),
        Err(e) => {
            log::error!("Everest fallback failed: {e:?}");
            match crate::schema::everest::request_everest(&serial).await {
                Ok(ev) => { 
                    log::info!("Everest info: {ev}"); 
                    Ok(crate::schema::prestashop::PrestashopPayload::default())
                },
                Err(e2) => Err(anyhow::anyhow!("Everest fallback failed: {e2:?}")),
            }
        },
    }
}

pub async fn get_order_from_prestashop_payload(serial: &str) -> anyhow::Result<crate::schema::prestashop::PrestashopPayload, anyhow::Error> {
    let api_call = Prestashop::default();
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[serial_number]", serial);
    query.insert("output_format", "JSON");
    let order_serials: Vec<crate::schema::prestashop::OrderSerialEntry> = api_call.request_resources_wasm("order_serial", query.clone()).await?;
    let id_order = order_serials
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("No id_order found"))?
        .id_order
        .clone();

    let payload = crate::schema::prestashop::PrestashopPayload::default().get_prestashop_payload(&id_order).await?;
    Ok(payload)
}

#[async_trait]
impl Task for TaskPayload {
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();
        let query = format!(
            "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
        );
        let get_data: Option<T> = DATABASE.query(query).await?.take(0)?;
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();
        let query = format!(
            "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
        );
        let get_data: Option<T> = DATABASE.query(query).await?.take(0)?;
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();
        let query = format!("SELECT * FROM task_note WHERE id={id}");
        let get_data: Option<T> = DATABASE.query(query).await?.take(0)?;
        debug!("get_data: {get_data:#?}");
        Ok(get_data)
    }

    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        let id: RecordId = self.id.clone();

        let get_data: Option<T> = DATABASE
                .query(
                    "SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id == $id"
                )
                .bind(("id", id))
                .await?
                .take(0)?;
        Ok(get_data)
    }
}
//! Done Shelf Audit
//! 
//! A cron job that audits services on the done-shelf (status 40) that:
//! - Have no notes/customer messages
//! - Have been on done-shelf for more than one day
//! 
//! For each matching service, it creates a task assigned to a specific user.

use anyhow::{Context, Result};
use chrono::{Duration, NaiveDateTime, Utc};
use database::{
    init_database,
    schema::{
        helper_traits::EmployeeHelper,
        prestashop_schema::{CustomerThread, Employee, Prestashop},
        utilities::create_full_task_payload,
        ComputerData, LiveTaskPayload, Priority, Store, TaskNotePayload,
        TicketData, User, TASK_TABLE, TICKET_TABLE,
    },
    DATABASE,
};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use simplelog::{ColorChoice, Config, LevelFilter, TermLogger, TerminalMode};
use std::collections::HashMap;
use surrealdb::RecordId;

/// Status code for done-shelf in Prestashop
const DONE_SHELF_STATUS: &str = "40";

/// Minimum days on done-shelf before flagging
const MIN_DAYS_ON_SHELF: i64 = 1;

/// Order with date information for filtering
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OrderWithDate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub date_add: String,
}


#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    TermLogger::init(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )?;

    info!("Starting Done Shelf Audit...");

    // Connect to database
    init_database().await?;

    // Run the audit
    let results = run_audit().await?;

    info!("Done Shelf Audit complete. Created {} tasks.", results);

    Ok(())
}

async fn run_audit() -> Result<usize> {
    let mut tasks_created = 0;
    
    // Get the assignee user
    let assignee: Option<User> = DATABASE
        .query("SELECT * FROM user WHERE id == $id")
        .bind(("id", RecordId::from(("user", "jm9a7l3v32gsiccr7pgw"))))
        .await?
        .take(0)?;

    let assignee = assignee.context("Could not find assignee user")?;
    info!("Tasks will be assigned to: {}", assignee.get_username());

    // Process each store using the Store enum
    for store in Store::VALUES {
        info!("Processing store {}...", store.as_str());
        
        match process_store(store, &assignee).await {
            Ok(count) => {
                tasks_created += count;
                info!("Store {}: created {} tasks", store.as_str(), count);
            }
            Err(e) => {
                error!("Error processing store {}: {:?}", store.as_str(), e);
            }
        }
    }

    Ok(tasks_created)
}

async fn process_store(store: Store, assignee: &User) -> Result<usize> {
    let mut tasks_created = 0;

    // Get all done-shelf services for this store with date info
    let orders = get_done_shelf_orders(store).await?;
    info!("Found {} orders on done-shelf for store {}", orders.len(), store.as_str());

    for order in orders {
        // Check if order has been on done-shelf for more than MIN_DAYS_ON_SHELF
        if !is_old_enough(&order.date_add) {
            continue;
        }

        // Check if order has any notes
        if has_notes(&order.id).await? {
            continue;
        }

        // Check if task already exists
        if task_exists(&order.id).await? {
            info!("Task already exists for order {}, skipping", order.id);
            continue;
        }

        // Create task for this order
        match create_audit_task(&order.id, assignee).await {
            Ok(_) => {
                tasks_created += 1;
                info!("Created audit task for order {}", order.id);
            }
            Err(e) => {
                error!("Failed to create task for order {}: {:?}", order.id, e);
            }
        }
    }

    Ok(tasks_created)
}

async fn get_done_shelf_orders(store: Store) -> Result<Vec<OrderWithDate>> {
    let mut api = Prestashop::default();
    api.display = "[id,date_add]";
    
    let store_id = store.into_store_id().to_string();
    
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[current_state]", DONE_SHELF_STATUS);
    query.insert("filter[id_order_type]", "2");
    query.insert("filter[id_store]", &store_id);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");

    let orders: Vec<OrderWithDate> = api
        .request_resources_wasm("orders", query)
        .await
        .context("Failed to fetch done-shelf orders")?;

    Ok(orders)
}

fn is_old_enough(date_str: &str) -> bool {
    match NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        Ok(order_date) => {
            let now = Utc::now().naive_utc();
            let age = now.signed_duration_since(order_date);
            age > Duration::days(MIN_DAYS_ON_SHELF)
        }
        Err(e) => {
            warn!("Failed to parse date {}: {}", date_str, e);
            false
        }
    }
}

async fn has_notes(order_id: &str) -> Result<bool> {
    let api = Prestashop::default();
    
    // First check if there are any customer threads for this order
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[id_order]", order_id);
    query.insert("output_format", "JSON");

    let threads: Vec<CustomerThread> = api
        .request_resources_wasm("customer_threads", query)
        .await
        .unwrap_or_default();

    // If no threads, there are no notes
    if threads.is_empty() {
        return Ok(false);
    }

    // Check if any thread has customer messages
    for thread in threads {
        if !thread.associations.customer_messages.is_empty() {
            return Ok(true);
        }
    }

    // Also check our database for task notes
    let db_notes: Vec<TaskNotePayload> = DATABASE
        .query("SELECT * FROM task_note WHERE service_number == $service_number")
        .bind(("service_number", order_id))
        .await?
        .take(0)?;

    Ok(!db_notes.is_empty())
}

async fn task_exists(service_number: &str) -> Result<bool> {
    let existing: Vec<LiveTaskPayload> = DATABASE
        .query("SELECT * FROM task WHERE service_number == $service_number")
        .bind(("service_number", service_number))
        .await?
        .take(0)?;

    Ok(!existing.is_empty())
}

async fn create_audit_task(order_id: &str, assignee: &User) -> Result<()> {
    // Get the full prestashop payload for this order
    let payload = Employee::to_prestashop_payload(order_id)
        .await
        .context("Failed to get prestashop payload")?;

    // Extract data from the payload
    let customer_data = payload.customer.clone();
    let order = &payload.order;

    // Create ticket data
    let ticket_data = TicketData {
        id: RecordId::from((TICKET_TABLE, order_id.to_string())),
        service_number: order_id.to_string(),
        salesman: String::new(),
        sales_rep: String::new(),
        tech: String::new(),
        checkin_rep: String::new(),
        terms: order.payment.clone(),
        ticket_total: order.total_products_wt.clone(),
        doc_alias: order.order_type.clone(),
        checkin_notes: String::new(),
        customer: customer_data.id.clone(),
        computer: None,
        ..Default::default()
    };

    // Create task data with audit note
    let task_name = format!(
        "[AUDIT] No Notes - {} - {}",
        customer_data.name, order_id
    );

    let task_data = LiveTaskPayload {
        id: RecordId::from((TASK_TABLE, order_id.to_string())),
        task_name,
        service_number: Some(order_id.to_string()),
        service_ticket: Some(ticket_data.id.clone()),
        assignee: assignee.get_id(),
        priority: Priority::Normal,
        due_date: Utc::now().into(),
        completed: false,
        ..Default::default()
    };

    let computer_data = ComputerData::default();

    // Create the task
    let result = create_full_task_payload(
        ticket_data,
        customer_data,
        computer_data,
        task_data,
        vec![],
        false,
    )
    .await;

    match result {
        database::schema::TaskCreationResult::Created { service_number } => {
            info!("Successfully created audit task for {}", service_number);
            Ok(())
        }
        database::schema::TaskCreationResult::AlreadyExists { service_number } => {
            info!("Task already exists for {}", service_number);
            Ok(())
        }
        database::schema::TaskCreationResult::Updated { service_number } => {
            info!("Updated existing task for {}", service_number);
            Ok(())
        }
        database::schema::TaskCreationResult::Error { message } => {
            Err(anyhow::anyhow!("Failed to create task: {}", message))
        }
    }
}


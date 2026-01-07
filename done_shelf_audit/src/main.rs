//! Done Shelf Audit
//! 
//! A CLI tool for auditing services and tasks:
//! - Done shelf services with no notes
//! - In-repair services with no notes  
//! - Overdue tasks
//! 
//! For each matching item, it creates/updates a task assigned to a specific user.

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Utc, Weekday};
use clap::Parser;
use database::{
    DATABASE, init_database, schema::{
        helper_traits::PrestashopPayloadHelper,
        ComputerData, LiveTaskPayload, Priority, 
        Store, TASK_TABLE, TASK_NOTE_TABLE, TICKET_TABLE, TaskNotePayload, 
        TicketData, 
        User,
        prestashop::{Order, OrderType, OrderState, PrestashopPayload}, 
        prestashop_schema::{
            CustomerThread,
            Prestashop, 
            PrestashopOrderType
        }, 
        utilities::create_full_task_payload
    }
};
use log::{error, info, warn};
use simplelog::{ColorChoice, Config, LevelFilter, TermLogger, TerminalMode};
use std::collections::HashMap;
use surrealdb_types::{Datetime, RecordId};

////////////// TODO
/// - Add a way to audit sales orders that are supposed to be service orders
/// 

/// Minimum days on done-shelf/in-repair before flagging
const MIN_DAYS_ON_SHELF: i64 = 1;

/// Minimum days overdue for task audit
const MIN_DAYS_TASK_OVERDUE: i64 = 3;

/// The audit assignee email
const AUDIT_ASSIGNEE_EMAIL: &str = "logan.lees@pclaptops.com";

#[derive(Parser, Debug)]
#[command(name = "done_shelf_audit")]
#[command(about = "Audit services and tasks for follow-up", long_about = None)]
struct Args {
    /// Audit done-shelf services (status 40) with no notes
    #[arg(long, short = 'd')]
    done: bool,

    /// Audit in-repair services with no notes
    #[arg(long, short = 'r')]
    repair: bool,

    /// Audit both done-shelf and in-repair services
    #[arg(long, short = 'b')]
    both: bool,

    /// Audit overdue tasks (due date > 3 days ago)
    #[arg(long, short = 't')]
    tasks: bool,

    /// Run all audits (done-shelf, in-repair, and overdue tasks)
    #[arg(long, short = 'a')]
    all: bool,

    /// Process a specific store (e.g., RIV, AF, LTN, MUR, ORE, SAN, WJ). Defaults to RIV.
    #[arg(long, short = 's')]
    store: Option<String>,

    /// Dry run - don't actually create/update tasks, just show what would happen
    #[arg(long)]
    dry_run: bool,
}

/// Summary entry for dry run output
#[derive(Debug, Clone)]
struct DryRunEntry {
    service_number: String,
    customer_name: String,
    #[allow(dead_code)]
    action: DryRunAction,
    days_overdue: i64,
    current_assignee: Option<String>,
    status: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum DryRunAction {
    CreateTask,
    UpdateTask,
    ReassignOverdueTask,
}

/// Summary for dry run
#[derive(Debug, Default)]
struct DryRunSummary {
    orders_found: Vec<DryRunEntry>,
    tasks_to_create: Vec<DryRunEntry>,
    tasks_to_update: Vec<DryRunEntry>,
    overdue_tasks: Vec<DryRunEntry>,
}

impl DryRunSummary {
    fn print_summary(&self) {
        println!("\n{}", "=".repeat(80));
        println!("                         🔍 DRY RUN SUMMARY");
        println!("{}\n", "=".repeat(80));

        // Orders found
        println!("📦 ORDERS FOUND (No Notes, Old Enough): {}", self.orders_found.len());
        println!("{}", "-".repeat(80));
        if self.orders_found.is_empty() {
            println!("   (none)");
        } else {
            for entry in &self.orders_found {
                println!("   • Service #{:<10} | {} | {} days | Status: {}", 
                    entry.service_number, 
                    truncate_string(&entry.customer_name, 25),
                    entry.days_overdue,
                    entry.status
                );
            }
        }
        println!();

        // Tasks to create
        println!("➕ TASKS TO CREATE: {}", self.tasks_to_create.len());
        println!("{}", "-".repeat(80));
        if self.tasks_to_create.is_empty() {
            println!("   (none)");
        } else {
            for entry in &self.tasks_to_create {
                println!("   • Service #{:<10} | {} | {} days overdue", 
                    entry.service_number, 
                    truncate_string(&entry.customer_name, 30),
                    entry.days_overdue
                );
            }
        }
        println!();

        // Tasks to update
        println!("🔄 EXISTING TASKS TO UPDATE: {}", self.tasks_to_update.len());
        println!("{}", "-".repeat(80));
        if self.tasks_to_update.is_empty() {
            println!("   (none)");
        } else {
            for entry in &self.tasks_to_update {
                println!("   • Service #{:<10} | {} | {} days | Current: {}", 
                    entry.service_number, 
                    truncate_string(&entry.customer_name, 20),
                    entry.days_overdue,
                    entry.current_assignee.as_deref().unwrap_or("Unknown")
                );
            }
        }
        println!();

        // Overdue tasks to reassign
        println!("⏰ OVERDUE TASKS TO REASSIGN: {}", self.overdue_tasks.len());
        println!("{}", "-".repeat(80));
        if self.overdue_tasks.is_empty() {
            println!("   (none)");
        } else {
            for entry in &self.overdue_tasks {
                println!("   • Service #{:<10} | {} | {} days overdue | Current: {}", 
                    entry.service_number, 
                    truncate_string(&entry.customer_name, 20),
                    entry.days_overdue,
                    entry.current_assignee.as_deref().unwrap_or("Unknown")
                );
            }
        }
        println!();

        // Totals
        let total = self.tasks_to_create.len() + self.tasks_to_update.len() + self.overdue_tasks.len();
        println!("{}", "=".repeat(80));
        println!("📊 TOTAL ACTIONS THAT WOULD BE TAKEN: {}", total);
        println!("   • New tasks to create:     {}", self.tasks_to_create.len());
        println!("   • Existing tasks to update: {}", self.tasks_to_update.len());
        println!("   • Overdue tasks to reassign: {}", self.overdue_tasks.len());
        println!("{}", "=".repeat(80));
        println!();
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        format!("{:<width$}", s, width = max_len)
    } else {
        format!("{}...", &s[..max_len - 3])
    }
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

    let args = Args::parse();

    // Validate that at least one audit type is selected
    if !args.done && !args.repair && !args.both && !args.tasks && !args.all {
        eprintln!("Error: Please specify at least one audit type.");
        eprintln!("Use --help for usage information.");
        eprintln!("\nExamples:");
        eprintln!("  done_shelf_audit --done          # Audit done-shelf services");
        eprintln!("  done_shelf_audit --repair        # Audit in-repair services");
        eprintln!("  done_shelf_audit --both          # Audit both done-shelf and in-repair");
        eprintln!("  done_shelf_audit --tasks         # Audit overdue tasks");
        eprintln!("  done_shelf_audit --all           # Run all audits");
        eprintln!("  done_shelf_audit --both -s RIV    # Audit both for RIV store");
        eprintln!("  done_shelf_audit --all --dry-run # See what would happen without making changes");
        std::process::exit(1);
    }

    info!("Starting Audit Tool...");
    if args.dry_run {
        info!("🔸 DRY RUN MODE - No changes will be made");
    }

    // Connect to database
    init_database().await?;

    // Get and validate the assignee user
    let assignee = get_audit_assignee().await?;
    info!("Tasks will be assigned to: {} ({})", assignee.get_name(), assignee.get_username());

    // Determine which stores to process - default to RIV unless -s is specified
    let stores = if let Some(store_str) = &args.store {
        match store_str.to_uppercase().as_str() {
            "RIV" => vec![Store::RIV],
            "LTN" => vec![Store::LTN],
            "MUR" => vec![Store::MUR],
            "ORE" => vec![Store::ORE],
            "SAN" => vec![Store::SAN],
            "WJ" => vec![Store::WJ],
            "ALL" => Store::VALUES.to_vec(),
            _ => {
                error!("Unknown store: {}. Valid stores: RIV, AF, LTN, MUR, ORE, SAN, WJ, ALL", store_str);
                std::process::exit(1);
            }
        }
    } else {
        // Default to RIV
        vec![Store::RIV]
    };

    info!("Processing store(s): {:?}", stores.iter().map(|s| s.as_str()).collect::<Vec<_>>());

    let mut total_processed = 0;
    let mut dry_run_summary = DryRunSummary::default();

    // Determine which audits to run
    let run_done = args.done || args.both || args.all;
    let run_repair = args.repair || args.both || args.all;
    let run_tasks = args.tasks || args.all;

    // Run service audits (done-shelf and/or in-repair)
    if run_done || run_repair {
        for store in &stores {
            info!("Processing store {}...", store.as_str());
            
            match run_service_audit(*store, &assignee, run_done, run_repair, args.dry_run, &mut dry_run_summary).await {
                Ok(count) => {
                    total_processed += count;
                    if !args.dry_run {
                        info!("Store {}: processed {} services", store.as_str(), count);
                    }
                }
                Err(e) => {
                    error!("Error processing store {}: {:?}", store.as_str(), e);
                }
            }
        }
    }

    // Run task audit
    if run_tasks {
        info!("Running task audit for overdue tasks...");
        match run_task_audit(&assignee, args.dry_run, &mut dry_run_summary).await {
            Ok(count) => {
                total_processed += count;
                if !args.dry_run {
                    info!("Task audit: processed {} overdue tasks", count);
                }
            }
            Err(e) => {
                error!("Error in task audit: {:?}", e);
            }
        }
    }

    // Print dry run summary if in dry run mode
    if args.dry_run {
        dry_run_summary.print_summary();
    } else {
        info!("Audit complete. Total processed: {}", total_processed);
    }

    Ok(())
}

/// Get the audit assignee user, ensuring they exist
async fn get_audit_assignee() -> Result<User> {
    let assignee: Option<User> = DATABASE
        .query("SELECT * FROM user WHERE email == $email")
        .bind(("email", AUDIT_ASSIGNEE_EMAIL.to_string()))
        .await?
        .take(0)?;

    assignee.context(format!(
        "Could not find audit assignee user with email: {}. Please ensure this user exists in the database.",
        AUDIT_ASSIGNEE_EMAIL
    ))
}

/// Run service audit (done-shelf and/or in-repair)
async fn run_service_audit(
    store: Store, 
    assignee: &User, 
    include_done: bool, 
    include_repair: bool,
    dry_run: bool,
    summary: &mut DryRunSummary,
) -> Result<usize> {
    let mut tasks_processed = 0;
    let mut orders = Vec::new();

    if include_done {
        let done_shelf_orders = get_done_shelf_orders(store).await?;
        info!("Found {} done-shelf orders for store {}", done_shelf_orders.len(), store.as_str());
        orders.extend(done_shelf_orders.into_iter().map(|o| (o, "Done Shelf")));
    }

    if include_repair {
        let in_repair_orders = get_in_repair_orders(store).await?;
        info!("Found {} in-repair orders for store {}", in_repair_orders.len(), store.as_str());
        orders.extend(in_repair_orders.into_iter().map(|o| (o, "In Repair")));
    }

    for (order, status) in orders {
        // Check if order has been on shelf for more than MIN_DAYS_ON_SHELF
        if !is_old_enough(&order.date_add, MIN_DAYS_ON_SHELF) {
            continue;
        }

        // Check if order has any notes - we only want orders with ZERO notes
        if has_notes(&order.id).await? {
            continue;
        }

        let days_overdue = get_days_overdue(&order.date_add);

        // For dry run, we need to get customer info
        let customer_name = if dry_run {
            get_customer_name_for_order(&order.id).await.unwrap_or_else(|_| "Unknown".to_string())
        } else {
            String::new()
        };

        // Add to orders found for summary
        if dry_run {
            summary.orders_found.push(DryRunEntry {
                service_number: order.id.clone(),
                customer_name: customer_name.clone(),
                action: DryRunAction::CreateTask, // placeholder
                days_overdue,
                current_assignee: None,
                status: status.to_string(),
            });
        }

        // Validate that assignee exists before proceeding
        if !dry_run && get_user_by_id(&assignee.get_id()).await?.is_none() {
            error!("Assignee user no longer exists! Aborting.");
            return Err(anyhow::anyhow!("Assignee user does not exist"));
        }

        // Check if task already exists
        if let Some(existing_task) = get_existing_task(&order.id).await? {
            if dry_run {
                let current_assignee = get_user_by_id(&existing_task.assignee).await?
                    .map(|u| u.get_username().to_string());
                
                summary.tasks_to_update.push(DryRunEntry {
                    service_number: order.id.clone(),
                    customer_name,
                    action: DryRunAction::UpdateTask,
                    days_overdue,
                    current_assignee,
                    status: status.to_string(),
                });
                tasks_processed += 1;
            } else {
                match update_existing_task(&existing_task, &order.id, &order.date_add, assignee, AuditType::Service).await {
                    Ok(_) => {
                        tasks_processed += 1;
                        info!("Updated existing task for order {}", order.id);
                    }
                    Err(e) => {
                        error!("Failed to update task for order {}: {:?}", order.id, e);
                    }
                }
            }
        } else {
            if dry_run {
                summary.tasks_to_create.push(DryRunEntry {
                    service_number: order.id.clone(),
                    customer_name,
                    action: DryRunAction::CreateTask,
                    days_overdue,
                    current_assignee: None,
                    status: status.to_string(),
                });
                tasks_processed += 1;
            } else {
                match create_audit_task(&order.id, &order.date_add, assignee).await {
                    Ok(_) => {
                        tasks_processed += 1;
                        info!("Created audit task for order {}", order.id);
                    }
                    Err(e) => {
                        error!("Failed to create task for order {}: {:?}", order.id, e);
                    }
                }
            }
        }
    }

    Ok(tasks_processed)
}

/// Get customer name for an order (used in dry run)
async fn get_customer_name_for_order(order_id: &str) -> Result<String> {
    let payload: PrestashopPayload = PrestashopPayload::default()
        .get_prestashop_payload(order_id)
        .await?;
    Ok(payload.customer.name)
}

/// Run task audit for overdue tasks
async fn run_task_audit(assignee: &User, dry_run: bool, summary: &mut DryRunSummary) -> Result<usize> {
    let mut tasks_processed = 0;

    // Get all incomplete tasks with due dates more than 3 days ago
    // Exclude tasks already assigned to the audit user
    let overdue_tasks: Vec<LiveTaskPayload> = DATABASE
        .query("SELECT * FROM task WHERE completed == false AND due_date < $cutoff_date AND assignee != $audit_user")
        .bind(("cutoff_date", Utc::now() - Duration::days(MIN_DAYS_TASK_OVERDUE)))
        .bind(("audit_user", assignee.get_id()))
        .await?
        .take(0)?;

    info!("Found {} overdue tasks (> {} days)", overdue_tasks.len(), MIN_DAYS_TASK_OVERDUE);

    for task in overdue_tasks {
        // Get task's due date as string for description building
        let due_date_str = task.due_date.to_string();
        let days_overdue = get_days_overdue(&due_date_str);

        if dry_run {
            let current_assignee = get_user_by_id(&task.assignee).await?
                .map(|u| u.get_username().to_string());
            
            let customer_name = if let Some(ref svc) = task.service_number {
                get_customer_name_for_order(svc).await.unwrap_or_else(|_| task.task_name.clone())
            } else {
                task.task_name.clone()
            };

            summary.overdue_tasks.push(DryRunEntry {
                service_number: task.service_number.clone().unwrap_or_else(|| "N/A".to_string()),
                customer_name,
                action: DryRunAction::ReassignOverdueTask,
                days_overdue,
                current_assignee,
                status: "Overdue".to_string(),
            });
            tasks_processed += 1;
        } else {
            // Validate that assignee exists before proceeding
            if get_user_by_id(&assignee.get_id()).await?.is_none() {
                error!("Assignee user no longer exists! Aborting.");
                return Err(anyhow::anyhow!("Assignee user does not exist"));
            }

            // Check if this task has an associated service order
            let audit_type = if let Some(ref service_number) = task.service_number {
                // Check if the order is AcceptedByOdoo
                if let Ok(Some(order_state)) = get_order_state(service_number).await {
                    if order_state == OrderState::AcceptedByOdoo {
                        AuditType::TaskAcceptedByOdoo
                    } else {
                        AuditType::Task
                    }
                } else {
                    AuditType::Task
                }
            } else {
                AuditType::Task
            };

            match update_existing_task(&task, task.service_number.as_deref().unwrap_or("N/A"), &due_date_str, assignee, audit_type).await {
                Ok(_) => {
                    tasks_processed += 1;
                    info!("Reassigned overdue task {:?} to audit user", task.id);
                }
                Err(e) => {
                    error!("Failed to reassign task {:?}: {:?}", task.id, e);
                }
            }
        }
    }

    Ok(tasks_processed)
}

/// Get the order state for a service number
async fn get_order_state(service_number: &str) -> Result<Option<OrderState>> {
    let api = Prestashop::default();
    
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[id]", service_number);
    query.insert("output_format", "JSON");

    let orders: Vec<Order> = api
        .request_resources_wasm("orders", query)
        .await
        .unwrap_or_default();

    if let Some(order) = orders.first() {
        Ok(Some(OrderState::state_from_id_str(&order.current_state)))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AuditType {
    Service,
    Task,
    TaskAcceptedByOdoo,
}

async fn get_done_shelf_orders(store: Store) -> Result<Vec<Order>> {
    let api = Prestashop::default();
    let store_id = store.into_store_id().to_string();
    let order_type = OrderType::ServiceOrder.to_id().to_string();
    
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[current_state]", PrestashopOrderType::DoneShelf.id());
    query.insert("filter[id_order_type]", &order_type);
    query.insert("filter[id_store]", &store_id);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");

    let orders: Vec<Order> = api
        .request_resources_wasm("orders", query)
        .await?;

    Ok(orders)
}

async fn get_in_repair_orders(store: Store) -> Result<Vec<Order>> {
    let api = Prestashop::default();
    let store_id = store.into_store_id().to_string();
    let order_type = OrderType::ServiceOrder.to_id().to_string();
    
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[current_state]", PrestashopOrderType::InRepair.id());
    query.insert("filter[id_order_type]", &order_type);
    query.insert("filter[id_store]", &store_id);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");

    let orders: Vec<Order> = api
        .request_resources_wasm("orders", query)
        .await?;

    Ok(orders)
}

fn is_old_enough(date_str: &str, min_days: i64) -> bool {
    match NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        Ok(order_date) => {
            let business_days = count_business_days_since(order_date.date());
            business_days > min_days
        }
        Err(e) => {
            warn!("Failed to parse date {}: {}", date_str, e);
            false
        }
    }
}

/// Count business days (excluding Sundays) since a given date
fn count_business_days_since(start_date: NaiveDate) -> i64 {
    let today = Utc::now().naive_utc().date();
    let mut count = 0i64;
    let mut current = start_date;
    
    while current < today {
        current += Duration::days(1);
        // Exclude Sundays
        if current.weekday() != Weekday::Sun {
            count += 1;
        }
    }
    
    count
}

/// Get all the dates where calls were missed (excluding Sundays)
fn get_missed_call_days(date_str: &str) -> Vec<String> {
    let mut missed_days = Vec::new();
    
    if let Ok(order_date) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        let today = Utc::now().naive_utc().date();
        let mut current = order_date.date();
        
        while current < today {
            current += Duration::days(1);
            // Exclude Sundays
            if current.weekday() != Weekday::Sun {
                missed_days.push(current.format("%A, %B %d").to_string());
            }
        }
    }
    
    missed_days
}

/// Get the number of business days overdue (excluding Sundays)
fn get_days_overdue(date_str: &str) -> i64 {
    if let Ok(order_date) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        count_business_days_since(order_date.date())
    } else {
        0
    }
}

/// Fetch a user by their RecordId
async fn get_user_by_id(user_id: &RecordId) -> Result<Option<User>> {
    let user: Option<User> = DATABASE
        .query("SELECT * FROM user WHERE id == $id")
        .bind(("id", user_id.clone()))
        .await?
        .take(0)?;
    
    Ok(user)
}

/// Build the audit description for services
fn build_service_audit_description(
    order_id: &str,
    date_str: &str,
    previous_assignee: Option<&str>,
    sales_rep: Option<&str>,
) -> String {
    let days_overdue = get_days_overdue(date_str);
    let missed_days = get_missed_call_days(date_str);
    
    let mut description = String::new();
    
    description.push_str(&format!("🔴 AUDIT - Service #{}\n\n", order_id));
    description.push_str("📋 REASON: No notes/customer contact after being on Done Shelf\n\n");
    
    if let Some(sales) = sales_rep {
        if !sales.is_empty() {
            description.push_str(&format!("🏷️ SALES REP: {}\n\n", sales));
        }
    }
    
    if let Some(prev_assignee) = previous_assignee {
        description.push_str(&format!("👤 PREVIOUS ASSIGNEE: {}\n\n", prev_assignee));
    }
    
    description.push_str(&format!("⏰ DAYS OVERDUE: {} business day(s)\n\n", days_overdue));
    
    if !missed_days.is_empty() {
        description.push_str("📅 MISSED CALL DAYS:\n");
        for day in &missed_days {
            description.push_str(&format!("  • {}\n", day));
        }
    }
    
    description
}

/// Build the audit description for overdue tasks
fn build_task_audit_description(
    task_id: &RecordId,
    service_number: Option<&str>,
    previous_assignee: Option<&str>,
    days_overdue: i64,
    recommend_complete: bool,
) -> String {
    let mut description = String::new();
    
    description.push_str(&format!("🔴 TASK AUDIT - {:?}\n\n", task_id));
    
    if let Some(svc) = service_number {
        description.push_str(&format!("📄 SERVICE: #{}\n\n", svc));
    }
    
    description.push_str("📋 REASON: Task overdue - no progress for extended period\n\n");
    
    if let Some(prev_assignee) = previous_assignee {
        description.push_str(&format!("👤 PREVIOUS ASSIGNEE: {}\n\n", prev_assignee));
    }
    
    description.push_str(&format!("⏰ DAYS OVERDUE: {} day(s)\n\n", days_overdue));
    
    if recommend_complete {
        description.push_str("✅ RECOMMENDATION: Service is marked as 'Accepted By Odoo' - consider completing this task.\n\n");
    }
    
    description
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
        .bind(("service_number", order_id.to_string()))
        .await?
        .take(0)?;

    Ok(!db_notes.is_empty())
}

async fn get_existing_task(service_number: &str) -> Result<Option<LiveTaskPayload>> {
    let existing: Option<LiveTaskPayload> = DATABASE
        .query("SELECT * FROM task WHERE service_number == $service_number")
        .bind(("service_number", service_number.to_string()))
        .await?
        .take(0)?;

    Ok(existing)
}

async fn update_existing_task(
    task: &LiveTaskPayload, 
    order_id: &str, 
    date_str: &str, 
    assignee: &User,
    audit_type: AuditType,
) -> Result<()> {
    let today: Datetime = Utc::now().into();
    
    // Get the previous assignee's username
    let previous_assignee_username = if let Some(prev_user) = get_user_by_id(&task.assignee).await? {
        prev_user.get_username().to_string()
    } else {
        "Unknown".to_string()
    };
    
    // Validate the new assignee exists
    let audit_user = get_user_by_id(&assignee.get_id()).await?
        .context("Audit assignee user does not exist")?;
    
    let audit_user_id = audit_user.get_id();
    
    // Build the appropriate audit description based on type
    let audit_description = match audit_type {
        AuditType::Service => {
            build_service_audit_description(order_id, date_str, Some(&previous_assignee_username), None)
        }
        AuditType::Task | AuditType::TaskAcceptedByOdoo => {
            let days_overdue = get_days_overdue(date_str);
            build_task_audit_description(
                &task.id,
                task.service_number.as_deref(),
                Some(&previous_assignee_username),
                days_overdue,
                audit_type == AuditType::TaskAcceptedByOdoo,
            )
        }
    };
    
    // Update the task
    DATABASE
        .query("UPDATE $task_id SET assignee = $assignee, due_date = $due_date")
        .bind(("task_id", task.id.clone()))
        .bind(("assignee", audit_user_id.clone()))
        .bind(("due_date", today))
        .await?;

    // Create a private task note explaining the reassignment
    let mut task_note = TaskNotePayload {
        id: RecordId::new(TASK_NOTE_TABLE, uuid::Uuid::new_v4().to_string().as_str()),
        task_id: Some(task.id.clone()),
        created_at: Utc::now().into(),
        note: audit_description,
        username: audit_user.get_username().to_string(),
        id_customer_thread: None,
        id_customer_message: None,
        id_employee: audit_user.get_employee_id().map(|id| id.to_string()),
        user: audit_user_id,
        service_number: task.service_number.clone(),
        private: true, // Private note - won't go to Prestashop
    };
    
    task_note.create_task_note_in_db().await?;

    info!("Updated task {:?} - reassigned from {} and private audit note created", task.id, previous_assignee_username);
    Ok(())
}

async fn create_audit_task(order_id: &str, date_str: &str, assignee: &User) -> Result<()> {
    // Validate assignee exists before creating task
    let validated_assignee = get_user_by_id(&assignee.get_id()).await?
        .context("Assignee user does not exist")?;

    // Get the full prestashop payload for this order
    let payload: PrestashopPayload = PrestashopPayload::default().get_prestashop_payload(order_id)
        .await
        .context("Failed to get prestashop payload")?;

    // Extract data from the payload
    let customer_data = payload.customer.clone();
    let order = &payload.order;
    
    // Get sales rep name if available
    let sales_rep_name = payload.sales_rep.as_ref().map(|emp| {
        format!("{} {}", emp.firstname, emp.lastname)
    });

    // Create ticket data
    let ticket_data = TicketData {
        id: RecordId::new(TICKET_TABLE, order_id),
        service_number: order_id.to_string(),
        salesman: String::new(),
        sales_rep: sales_rep_name.clone().unwrap_or_default(),
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

    // Build the audit description including sales rep
    let audit_description = build_service_audit_description(
        order_id, 
        date_str, 
        None, 
        sales_rep_name.as_deref()
    );

    // Create task data with audit description
    let task_name = format!(
        "[AUDIT] No Notes - {} - {}",
        customer_data.name, order_id
    );

    let task_data = LiveTaskPayload {
        id: RecordId::new(TASK_TABLE, order_id.to_string()),
        task_name,
        service_number: Some(order_id.to_string()),
        service_ticket: Some(ticket_data.id.clone()),
        assignee: validated_assignee.get_id(),
        priority: Priority::Normal,
        due_date: Utc::now().into(),
        completed: false,
        task_description: audit_description,
        status: database::schema::Status::Todo,
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
            log::warn!("Task already exists for {}", service_number);
            Ok(())
        }
        database::schema::TaskCreationResult::Updated { service_number } => {
            log::warn!("Updated existing task for {}", service_number);
            Ok(())
        }
        database::schema::TaskCreationResult::Error { message } => {
            Err(anyhow::anyhow!("Failed to create task: {}", message))
        }
    }
}

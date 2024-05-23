use axum::{Extension, Json};
use log::debug;
use serde_json::Value;
use surrealdb::sql::Thing;
use database::{schema::{ComputerData, ComputerId, CustomerData, CustomerId, LocalSebData, Priority, Record, RecordResult, Status, TaskPayload, TicketData, TicketId, TicketPayload, UserId, COMPUTER_TABLE, CUSTOMER_TABLE, TICKET_TABLE}, Database};
use crate::{
    middlewares::context::Ctx, routes::{api::tickets::create::insert_ticket, user::query_user_from_initials}, utils::error::{
        ApiError,
        Error
    }
};

use self::get::query_ticket;

pub mod create;
pub mod get;
pub mod update;
pub mod delete;

pub async fn handle_create_ticket(
    db: Extension<Database>, 
    ctx: Ctx,
    Json(payload): Json<TicketPayload>
) -> Json<Result<RecordResult, ApiError>> { 

    let cust_payload: CustomerData = payload.customer_data;
    let ticket_payload: TicketData = payload.ticket_data;
    let computer_payload: ComputerData = payload.computer_data;

    let customer_id: CustomerId = CustomerId(Thing::from((CUSTOMER_TABLE.to_string(), cust_payload.cust_code.to_string())));
    let ticket_id: TicketId = TicketId(Thing::from((TICKET_TABLE.to_string(), ticket_payload.service_number.to_string())));
    // let task_id = TaskId(Thing::from((TASK_TABLE.to_string(), format!("{}-{}", ticket_payload.service_number.clone(), Uuid::new_v4()))));

    // Just the computer hostname is not enough for a unique identifier so i am concatting the {hostname-cust_code}
    let computer_customer_id: String = format!("{}-{}", computer_payload.hostname, cust_payload.cust_code);
    let computer_id: ComputerId = ComputerId(Thing::from((COMPUTER_TABLE.to_string() , computer_customer_id)));

    let queried_user: Option<UserId> = query_user_from_initials(
        db.0.clone(), 
        Some(ticket_payload.salesman.clone()),
        None
    ).await.unwrap();

    let local_seb_data: Option<LocalSebData>;

    if let Some(seb_data) = computer_payload.seb_info{
        local_seb_data = Some( LocalSebData {
            InstalledDeviceId: seb_data.InstalledDeviceId,
            InstallInstanceId: seb_data.InstallInstanceId,
            HasIssues: seb_data.HasIssues,
            InstallationStage: seb_data.InstallationStage,
            ReasonCode: seb_data.ReasonCode,
            ActivationCode: seb_data.ActivationCode,
            InstallVersion: seb_data.InstallVersion,
            MachineName: seb_data.MachineName,
            ExtendedSeb: seb_data.ExtendedSeb
        });
    }else{ local_seb_data = None; }

    let mut owned_computers: Vec<ComputerId> = Vec::new();
    let mut services: Vec<TicketId> = Vec::new();

    owned_computers.push(computer_id.clone());
    services.push(ticket_id.clone());

    let cust_data = CustomerData{
        id: Some(customer_id.clone()),
        computers: Some(owned_computers),
        services: Some(services),
        cust_code: cust_payload.cust_code,
        name: cust_payload.name.clone(),
        phone_number: cust_payload.phone_number,
        phone_number_2: cust_payload.phone_number_2,
        email: cust_payload.email,
        li_doc: cust_payload.li_doc,
        li_amnt: cust_payload.li_amnt,
        num_inv: cust_payload.num_inv,
        part_order_links: None
    };
    
    let ticket_data = TicketData {
        id: Some(ticket_id.clone()),
        due_date: ticket_payload.due_date.clone(),
        customer: Some(customer_id.clone()),
        computer: Some(computer_id.clone()),
        service_task: None,
        created_at: None,
        service_number: ticket_payload.service_number,
        checkin_rep: ticket_payload.checkin_rep,
        checkin_notes: ticket_payload.checkin_notes,
        recommendations: ticket_payload.recommendations,
        tech: ticket_payload.tech.clone(),
        salesman: ticket_payload.salesman.clone(),
        dep: ticket_payload.dep.clone(),
        terms: ticket_payload.terms,
        ticket_total: ticket_payload.ticket_total,
        doc_alias: ticket_payload.doc_alias,
        current_antivirus: ticket_payload.current_antivirus,
        hardware_test_results: ticket_payload.hardware_test_results,
        sales_rep: ticket_payload.sales_rep.clone(),
    };

    let computer_data = ComputerData{
        id: Some(computer_id),
        customer: Some(customer_id),
        seb_info: local_seb_data,
        hostname: computer_payload.hostname,
        operating_system: computer_payload.operating_system,
        cpu: computer_payload.cpu,
        gpu: computer_payload.gpu,
        ram: computer_payload.ram,
        drives: computer_payload.drives,
    };

    let task_name = format!("{} - {}", cust_payload.name, ticket_payload.service_number);

    let task_data = TaskPayload{
        id: None,
        task_name,
        service_ticket: Some(ticket_id),
        task_description: None,
        // assignee_name: Some(queried_user.name),
        // assignee_email: queried_user.email,
        assignee: queried_user,
        service_number: Some(ticket_payload.service_number),
        due_date: ticket_payload.due_date,
        priority: Some(Priority::Normal),
        task_note: None,
        completed: false,
        assignee_initials: Some(ticket_payload.salesman),
        status: Status::Todo,
        assignee_email: None,
        dep: Some(ticket_payload.dep)
    };

    debug!("\n\nCustomer --> {cust_data:#?}\n\n");
    debug!("\n\nTicket --> {ticket_data:#?}\n\n");
    debug!("\n\nComputer --> {computer_data:#?}\n\n");
    debug!("\n\nTask --> {task_data:#?}\n\n");

    let insert_data = insert_ticket(
        db.0,
        ticket_data,
        cust_data,
        computer_data,
        task_data,
    ).await;

    let res = insert_data
        .or_else(|err|{
            Err(ApiError{
                error: Error::Generic { description: err.to_string() },
                req_id: ctx.req_id()
            })
        }).and_then(|rec|{
            let x = format!("Returned records: {rec:#?}");
            debug!("{x:?}");
            Ok(RecordResult{
                result: true,
                record: Some(x)
            })
        });

    Json(res)
}


pub async fn handle_get_ticket(
    db: Extension<Database>,
    _ctx: Ctx,
) -> Json<Result<Vec<Value>, Error>> { 
    let x: Result<Vec<Value>, Error> = query_ticket(db.0)
        .await;
    Json(x)
}


pub async fn handle_update_spo(
    db: Extension<Database>,
    _ctx: Ctx,
    spo_link: String
) -> Json<Result<Vec<Record>, surrealdb::Error>> { 
    let query = format!("UPDATE customer SET part_order_links += [{spo_link}]");

    let res: Result<Vec<Record>, surrealdb::Error> = db.0.sql(query.as_str()).await;

    Json(res)
}
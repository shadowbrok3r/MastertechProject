use super::{ComputerData, CustomerData, HardwareTests, Job, CUSTOMER_TABLE, TICKET_TABLE};
use surrealdb::{sql::Datetime, RecordId};
use structdiff::{Difference, StructDiff};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TicketPayload {
    pub id: RecordId,
    pub created_at: Datetime,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub service_number: String,
    /// Person that checked computer in
    pub checkin_rep: String,
    pub sales_rep: String,
    pub checkin_notes: String,
    pub tech: String,
    pub salesman: String,
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
    pub jobs: Option<Vec<Job>>
}

impl Default for TicketPayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TICKET_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            created_at: Default::default(),
            customer: Default::default(),
            computer: Default::default(),
            service_number: Default::default(),
            checkin_rep: Default::default(),
            sales_rep: Default::default(),
            checkin_notes: Default::default(),
            tech: Default::default(),
            salesman: Default::default(),
            terms: Default::default(),
            ticket_total: Default::default(),
            doc_alias: Default::default(),
            current_antivirus: Default::default(),
            hardware_test_results: Default::default(),
            jobs: Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TicketData {
    // Live Ticket Payload
    pub id: RecordId,
    pub created_at: Datetime,
    pub customer: RecordId,
    pub computer: Option<RecordId>,
    pub service_number: String,
    /// Person that checked computer in
    pub checkin_rep: String,
    pub sales_rep: String,
    pub checkin_notes: String,
    pub tech: String,
    pub salesman: String,
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
    pub jobs: Option<Vec<Job>>
}

impl Default for TicketData {
    fn default() -> Self {
        Self {
            id: RecordId::from((TICKET_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            customer: RecordId::from((CUSTOMER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            created_at: Default::default(),
            computer: Default::default(),
            service_number: Default::default(),
            checkin_rep: Default::default(),
            sales_rep: Default::default(),
            checkin_notes: Default::default(),
            tech: Default::default(),
            salesman: Default::default(),
            terms: Default::default(),
            ticket_total: Default::default(),
            doc_alias: Default::default(),
            current_antivirus: Default::default(),
            hardware_test_results: Default::default(),
            jobs: Default::default(),
        }
    }
}

impl From<TicketData> for TicketPayload {
    fn from(ticket: TicketData) -> Self {
        Self {
            id: ticket.id,
            created_at: ticket.created_at,
            service_number: ticket.service_number,
            checkin_rep: ticket.checkin_rep,
            sales_rep: ticket.sales_rep,
            checkin_notes: ticket.checkin_notes,
            tech: ticket.tech,
            salesman: ticket.salesman,
            terms: ticket.terms,
            ticket_total: ticket.ticket_total,
            doc_alias: ticket.doc_alias,
            current_antivirus: ticket.current_antivirus,
            hardware_test_results: ticket.hardware_test_results,
            ..Default::default()
        }
    }
}

impl From<TicketPayload> for TicketData {
    fn from(ticket: TicketPayload) -> Self {
        Self {
            id: ticket.id,
            created_at: ticket.created_at,
            service_number: ticket.service_number,
            checkin_rep: ticket.checkin_rep,
            sales_rep: ticket.sales_rep,
            checkin_notes: ticket.checkin_notes,
            tech: ticket.tech,
            salesman: ticket.salesman,
            terms: ticket.terms,
            ticket_total: ticket.ticket_total,
            doc_alias: ticket.doc_alias,
            current_antivirus: ticket.current_antivirus,
            hardware_test_results: ticket.hardware_test_results,
            customer: ticket.customer.unwrap_or_default().id,
            computer: Some(ticket.computer.unwrap_or_default().id),
            jobs: ticket.jobs,
        }
    }
}
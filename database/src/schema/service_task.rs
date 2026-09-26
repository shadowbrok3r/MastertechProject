//! Finds or creates the service `task` a service order's records attach to; writes Mastertech rows only.

use serde::{Deserialize, Serialize};

use super::{
    AgentThread, AssistRequest, Datetime, Priority, RecordId, Status, SurrealValue, TASK_TABLE,
};
use crate::db;

/// `task.origin` of a task the diagnostic agent created.
pub const AGENT_TASK_ORIGIN: &str = "ai";

/// Customer half of the task name when the order names no customer.
const UNKNOWN_CUSTOMER: &str = "Unknown customer";

/// A service number as orders and tasks store it, or `None` when `raw` cannot be one.
pub fn normalize_service_number(raw: &str) -> Option<String> {
    let sn = raw.trim().trim_start_matches('#').trim();
    let valid = !sn.is_empty()
        && sn.len() <= 32
        && sn
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| sn.to_string())
}

/// The first candidate that normalizes to a service number.
pub fn first_service_number<'a>(
    candidates: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .find_map(normalize_service_number)
}

/// `<customer> - <service number>`, the shop's task name.
pub fn service_task_name(customer_name: Option<&str>, service_number: &str) -> String {
    let customer = customer_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(UNKNOWN_CUSTOMER);
    format!("{customer} - {service_number}")
}

/// Description of an agent-created task: who it was opened for, then the check-in notes.
pub fn agent_task_description(
    requester: &str,
    service_number: &str,
    checkin_notes: Option<&str>,
) -> String {
    let mut out = format!(
        "Created by the Mastertech diagnostic agent for {requester}: service #{service_number} had no task."
    );
    if let Some(notes) = checkin_notes.map(str::trim).filter(|n| !n.is_empty()) {
        out.push_str("\nCheck-in notes: ");
        out.push_str(notes);
    }
    out
}

/// A task already filed under a service number or its service_order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct TaskCandidate {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub task_name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub completed: Option<bool>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
}

/// What `ensure_service_task` does for an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPlan {
    Reuse(RecordId),
    Create,
}

/// The task to reuse: an open one before a completed one, then the newest.
pub fn pick_existing_task(candidates: &[TaskCandidate]) -> Option<&TaskCandidate> {
    candidates.iter().min_by(|a, b| {
        a.completed
            .unwrap_or(false)
            .cmp(&b.completed.unwrap_or(false))
            .then_with(|| b.created_at.cmp(&a.created_at))
    })
}

/// Reuses any task the order already has, whatever its status; creates only when there is none.
pub fn plan_service_task(candidates: &[TaskCandidate]) -> TaskPlan {
    match pick_existing_task(candidates) {
        Some(task) => TaskPlan::Reuse(task.id.clone()),
        None => TaskPlan::Create,
    }
}

/// The service_order fields a task is built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct ServiceOrderRow {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub customer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub customer_name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub computer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub checkin_notes: Option<String>,
}

/// The technician an agent-created task is assigned to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct Requester {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub email: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
}

impl Requester {
    /// Display name, else the email's local part.
    pub fn label(&self) -> String {
        let name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        let local = self
            .email
            .as_deref()
            .and_then(|e| e.split('@').next())
            .map(str::trim)
            .filter(|l| !l.is_empty());
        name.or(local)
            .unwrap_or("the requesting technician")
            .to_string()
    }
}

/// The email forms a technician identifier can match: the address itself, or the username on every company domain.
pub fn requester_emails(ident: &str) -> Vec<String> {
    super::email_candidates(ident)
}

/// Content of a task the diagnostic agent files for a service order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct NewServiceTask {
    pub task_name: String,
    pub service_ticket: RecordId,
    pub service_number: String,
    pub task_description: String,
    pub assignee: RecordId,
    pub due_date: Datetime,
    pub priority: Priority,
    pub completed: bool,
    pub status: Status,
    pub created_at: Datetime,
    pub origin: String,
}

impl NewServiceTask {
    /// Open, normal-priority task on `order` for `requester`, due `now`.
    pub fn for_order(
        order: &ServiceOrderRow,
        service_number: &str,
        customer_name: Option<&str>,
        requester: &Requester,
        now: Datetime,
    ) -> Self {
        let customer = order.customer_name.as_deref().or(customer_name);
        Self {
            task_name: service_task_name(customer, service_number),
            service_ticket: order.id.clone(),
            service_number: service_number.to_string(),
            task_description: agent_task_description(
                &requester.label(),
                service_number,
                order.checkin_notes.as_deref(),
            ),
            assignee: requester.id.clone(),
            due_date: now,
            priority: Priority::Normal,
            completed: false,
            status: Status::Todo,
            created_at: now,
            origin: AGENT_TASK_ORIGIN.to_string(),
        }
    }
}

/// Inputs to `ensure_service_task` beyond the order itself.
#[derive(Debug, Clone, Default)]
pub struct ServiceTaskRequest {
    /// Technician email, username or name; required only when a task must be created.
    pub requested_by: Option<String>,
    /// Machine under diagnosis, written to the order when it has none.
    pub computer: Option<RecordId>,
    /// Customer of the machine, written to the order when it has none.
    pub customer: Option<RecordId>,
    /// Customer name used when the order's customer has none.
    pub customer_name: Option<String>,
}

/// What `ensure_service_task` settled on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnsuredServiceTask {
    pub task: RecordId,
    pub task_name: Option<String>,
    pub created: bool,
    pub order: ServiceOrderRow,
    /// Assignee of a created task.
    pub assignee: Option<Requester>,
    /// Links written onto the order: `computer`, `customer`.
    pub order_links_filled: Vec<&'static str>,
    /// Why filling the order's missing links failed, when it did.
    pub order_links_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceTaskError {
    #[error("no technician to assign the new task to: pass requested_by")]
    NoRequester,
    #[error("requested_by '{0}' matches no active Mastertech user")]
    UnknownRequester(String),
    #[error(transparent)]
    Db(#[from] anyhow::Error),
}

/// Service number and requester a machine's live agent thread or latest assist request carry.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EngagementHints {
    pub thread: Option<RecordId>,
    pub service_number: Option<String>,
    pub requested_by: Option<String>,
}

/// The first candidate with text after trimming.
fn first_text<'a>(candidates: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

impl EngagementHints {
    /// Thread fields first, then the request's.
    pub fn from_rows(thread: Option<&AgentThread>, request: Option<&AssistRequest>) -> Self {
        Self {
            thread: thread.map(|t| t.id.clone()),
            service_number: first_service_number([
                thread.and_then(|t| t.service_number.as_deref()),
                request.and_then(|r| r.service_number.as_deref()),
            ]),
            requested_by: first_text([
                thread.and_then(|t| t.requested_by.as_deref()),
                request.and_then(|r| r.requested_by.as_deref()),
            ]),
        }
    }
}

/// Engagement hints for a machine; lookup failures leave the hint empty.
pub async fn engagement_hints(connection_string: &str) -> EngagementHints {
    let thread = AgentThread::active_for_connection(connection_string)
        .await
        .unwrap_or_else(|e| {
            log::warn!("service_task: agent thread lookup failed for {connection_string}: {e}");
            None
        });
    let request =
        AssistRequest::latest_for_engagement(connection_string, thread.as_ref().map(|t| &t.id))
            .await
            .unwrap_or_else(|e| {
                log::warn!(
                    "service_task: assist request lookup failed for {connection_string}: {e}"
                );
                None
            });
    EngagementHints::from_rows(thread.as_ref(), request.as_ref())
}

/// The service_order row for `$sn`, with its customer's name.
pub const SERVICE_ORDER_BY_NUMBER_SQL: &str = "SELECT id, service_number, customer, \
     customer.name AS customer_name, computer, checkin_notes FROM service_order \
     WHERE service_number == $sn LIMIT 1";

/// Tasks filed under `$sn` or pointing at `$so`.
pub const TASKS_FOR_SERVICE_SQL: &str = "SELECT id, task_name, completed, created_at FROM task \
     WHERE service_number == $sn OR ($so != NONE AND service_ticket == $so)";

/// The active user whose lowercased email is in `$emails` or whose lowercased name is `$name`.
pub const REQUESTER_SQL: &str = "SELECT id, name, email, store FROM user \
     WHERE (string::lowercase(email ?? '') IN $emails OR string::lowercase(name ?? '') == $name) \
     AND active != false LIMIT 1";

/// Sets `$so`'s computer and customer where they are unset.
pub const FILL_ORDER_LINKS_SQL: &str =
    "UPDATE $so SET computer = computer ?? $computer, customer = customer ?? $customer";

/// The service_order for a service number, with its customer's name.
pub async fn service_order_by_number(
    service_number: &str,
) -> anyhow::Result<Option<ServiceOrderRow>> {
    let rows: Vec<ServiceOrderRow> = db()
        .query(SERVICE_ORDER_BY_NUMBER_SQL)
        .bind(("sn", service_number.to_string()))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next())
}

/// The service_order behind a record id, with its customer's name.
pub async fn service_order_by_id(id: &RecordId) -> anyhow::Result<Option<ServiceOrderRow>> {
    let rows: Vec<ServiceOrderRow> = db()
        .query(
            "SELECT id, service_number, customer, customer.name AS customer_name, computer, \
             checkin_notes FROM $id",
        )
        .bind(("id", id.clone()))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next())
}

/// Tasks filed under `service_number` or pointing at `service_order`.
pub async fn tasks_for_service(
    service_number: &str,
    service_order: Option<&RecordId>,
) -> anyhow::Result<Vec<TaskCandidate>> {
    let rows: Vec<TaskCandidate> = db()
        .query(TASKS_FOR_SERVICE_SQL)
        .bind(("sn", service_number.to_string()))
        .bind(("so", service_order.cloned()))
        .await?
        .take(0)?;
    Ok(rows)
}

/// The task an existing service number already has, open before completed.
pub async fn find_service_task(service_number: &str) -> anyhow::Result<Option<TaskCandidate>> {
    let Some(sn) = normalize_service_number(service_number) else {
        return Ok(None);
    };
    let order = service_order_by_number(&sn).await?;
    let candidates = tasks_for_service(&sn, order.as_ref().map(|o| &o.id)).await?;
    Ok(pick_existing_task(&candidates).cloned())
}

/// An active user matching an email, a company username or an exact name.
pub async fn resolve_requester(ident: &str) -> anyhow::Result<Option<Requester>> {
    if ident.trim().is_empty() {
        return Ok(None);
    }
    let emails = requester_emails(ident);
    let rows: Vec<Requester> = db()
        .query(REQUESTER_SQL)
        .bind(("emails", emails))
        .bind(("name", ident.trim().to_lowercase()))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next())
}

/// Writes the order's missing computer and customer links; set links are kept.
async fn fill_order_links(
    order: &ServiceOrderRow,
    request: &ServiceTaskRequest,
) -> (Vec<&'static str>, Option<String>) {
    let computer = request
        .computer
        .clone()
        .filter(|_| order.computer.is_none());
    let customer = request
        .customer
        .clone()
        .filter(|_| order.customer.is_none());
    let mut filled = Vec::new();
    if computer.is_some() {
        filled.push("computer");
    }
    if customer.is_some() {
        filled.push("customer");
    }
    if filled.is_empty() {
        return (filled, None);
    }
    let res = db()
        .query(FILL_ORDER_LINKS_SQL)
        .bind(("so", order.id.clone()))
        .bind(("computer", computer))
        .bind(("customer", customer))
        .await
        .and_then(|r| r.check());
    match res {
        Ok(_) => (filled, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    }
}

/// Writes `task` under a fresh random id.
async fn create_task(task: &NewServiceTask) -> anyhow::Result<RecordId> {
    let id = super::random_record_id(TASK_TABLE);
    let created: Option<super::Record> = db().create(id.clone()).content(task.clone()).await?;
    Ok(created.map(|r| r.id).unwrap_or(id))
}

/// The order's task: an existing one of any status, else a new one for the requester.
pub async fn ensure_service_task(
    order: &ServiceOrderRow,
    request: &ServiceTaskRequest,
) -> Result<EnsuredServiceTask, ServiceTaskError> {
    let service_number = order
        .service_number
        .as_deref()
        .and_then(normalize_service_number)
        .ok_or_else(|| anyhow::anyhow!("service_order {:?} carries no service number", order.id))?;
    let candidates = tasks_for_service(&service_number, Some(&order.id)).await?;
    let (task, task_name, created, assignee) = match plan_service_task(&candidates) {
        TaskPlan::Reuse(id) => {
            let name = candidates
                .iter()
                .find(|c| c.id == id)
                .and_then(|c| c.task_name.clone());
            (id, name, false, None)
        }
        TaskPlan::Create => {
            let ident = request
                .requested_by
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or(ServiceTaskError::NoRequester)?;
            let requester = resolve_requester(ident)
                .await?
                .ok_or_else(|| ServiceTaskError::UnknownRequester(ident.to_string()))?;
            let new = NewServiceTask::for_order(
                order,
                &service_number,
                request.customer_name.as_deref(),
                &requester,
                chrono::Utc::now().into(),
            );
            let id = create_task(&new).await?;
            log::info!("service_task: created {id:?} for #{service_number}");
            (id, Some(new.task_name), true, Some(requester))
        }
    };
    let (order_links_filled, order_links_error) = fill_order_links(order, request).await;
    if let Some(e) = &order_links_error {
        log::warn!("service_task: order links for #{service_number} not written: {e}");
    }
    Ok(EnsuredServiceTask {
        task,
        task_name,
        created,
        order: order.clone(),
        assignee,
        order_links_filled,
        order_links_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CUSTOMER_TABLE, TICKET_TABLE, USER_TABLE};

    fn at(secs: i64) -> Option<Datetime> {
        chrono::DateTime::from_timestamp(secs, 0).map(Datetime::from)
    }

    fn candidate(key: &str, completed: Option<bool>, created: Option<Datetime>) -> TaskCandidate {
        TaskCandidate {
            id: RecordId::new(TASK_TABLE, key),
            task_name: Some(format!("{key} - 2155467")),
            completed,
            created_at: created,
        }
    }

    fn order() -> ServiceOrderRow {
        ServiceOrderRow {
            id: RecordId::new(TICKET_TABLE, "2155467"),
            service_number: Some("2155467".into()),
            customer: Some(RecordId::new(CUSTOMER_TABLE, "4215")),
            customer_name: Some("Barbara Baker".into()),
            computer: None,
            checkin_notes: Some("CPS and annual tuneup".into()),
        }
    }

    fn derek() -> Requester {
        Requester {
            id: RecordId::new(USER_TABLE, "derek"),
            name: Some("Derek Anderson".into()),
            email: Some("derek.anderson@pclaptops.com".into()),
            store: Some("MUR".into()),
        }
    }

    #[test]
    fn service_numbers_normalize_or_are_refused() {
        assert_eq!(
            normalize_service_number(" 2155467 ").as_deref(),
            Some("2155467")
        );
        assert_eq!(
            normalize_service_number("#2155467").as_deref(),
            Some("2155467")
        );
        assert_eq!(
            normalize_service_number("SO-12345").as_deref(),
            Some("SO-12345")
        );
        for bad in [
            "",
            "   ",
            "#",
            "2155 467",
            "2155467;DELETE task",
            "21554`67",
        ] {
            assert_eq!(normalize_service_number(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_first_usable_service_number_wins() {
        assert_eq!(
            first_service_number([None, Some("  "), Some("#2155467"), Some("2155113")]).as_deref(),
            Some("2155467")
        );
        assert_eq!(first_service_number([None, Some("bad number")]), None);
    }

    #[test]
    fn task_names_follow_the_customer_dash_number_form() {
        assert_eq!(
            service_task_name(Some("Barbara Baker"), "2155467"),
            "Barbara Baker - 2155467"
        );
        assert_eq!(
            service_task_name(Some("  brayden humphreys "), "2155359"),
            "brayden humphreys - 2155359"
        );
        assert_eq!(
            service_task_name(Some(""), "2155467"),
            "Unknown customer - 2155467"
        );
        assert_eq!(
            service_task_name(None, "2155467"),
            "Unknown customer - 2155467"
        );
    }

    #[test]
    fn an_open_task_is_reused_before_a_completed_one() {
        let tasks = [
            candidate("done-new", Some(true), at(3_000)),
            candidate("open-old", Some(false), at(1_000)),
        ];
        assert_eq!(
            plan_service_task(&tasks),
            TaskPlan::Reuse(RecordId::new(TASK_TABLE, "open-old"))
        );
    }

    #[test]
    fn a_completed_task_is_still_reused_rather_than_duplicated() {
        let tasks = [candidate("done", Some(true), at(1_000))];
        assert_eq!(
            plan_service_task(&tasks),
            TaskPlan::Reuse(RecordId::new(TASK_TABLE, "done"))
        );
    }

    #[test]
    fn the_newest_task_wins_among_equals() {
        let tasks = [
            candidate("older", Some(false), at(1_000)),
            candidate("undated", None, None),
            candidate("newer", Some(false), at(2_000)),
        ];
        assert_eq!(
            plan_service_task(&tasks),
            TaskPlan::Reuse(RecordId::new(TASK_TABLE, "newer"))
        );
    }

    #[test]
    fn a_task_is_created_only_when_none_exists() {
        assert_eq!(plan_service_task(&[]), TaskPlan::Create);
    }

    #[test]
    fn a_new_task_carries_the_order_links_and_the_agent_origin() {
        let now = at(1_758_820_000).expect("valid timestamp");
        let task = NewServiceTask::for_order(&order(), "2155467", None, &derek(), now);
        assert_eq!(task.task_name, "Barbara Baker - 2155467");
        assert_eq!(task.service_ticket, order().id);
        assert_eq!(task.service_number, "2155467");
        assert_eq!(task.assignee, derek().id);
        assert_eq!(task.priority, Priority::Normal);
        assert_eq!(task.status, Status::Todo);
        assert!(!task.completed);
        assert_eq!(task.due_date, now);
        assert_eq!(task.created_at, now);
        assert_eq!(task.origin, AGENT_TASK_ORIGIN);
        assert_eq!(
            task.task_description,
            "Created by the Mastertech diagnostic agent for Derek Anderson: service #2155467 had no task.\nCheck-in notes: CPS and annual tuneup"
        );
    }

    #[test]
    fn a_new_task_stores_priority_and_status_as_the_create_task_flow_does() {
        use surrealdb_types::Value;
        let now = at(0).expect("epoch");
        let fields = |v: Value| match v {
            Value::Object(obj) => ["priority", "status", "completed"].map(|k| obj.get(k).cloned()),
            other => panic!("task should store as an object, got {other:?}"),
        };
        let agent = NewServiceTask::for_order(&order(), "2155467", None, &derek(), now);
        let modal = crate::schema::LiveTaskPayload::default();
        assert_eq!(
            fields(agent.clone().into_value()),
            fields(modal.into_value())
        );
        match agent.into_value() {
            Value::Object(obj) => assert_eq!(obj.get("origin"), Some(&Value::String("ai".into()))),
            other => panic!("task should store as an object, got {other:?}"),
        }
    }

    #[test]
    fn the_order_customer_name_beats_the_session_one() {
        let now = at(0).expect("epoch");
        let task =
            NewServiceTask::for_order(&order(), "2155467", Some("Session Name"), &derek(), now);
        assert_eq!(task.task_name, "Barbara Baker - 2155467");
        let nameless = ServiceOrderRow {
            customer_name: None,
            checkin_notes: None,
            ..order()
        };
        let task =
            NewServiceTask::for_order(&nameless, "2155467", Some("Session Name"), &derek(), now);
        assert_eq!(task.task_name, "Session Name - 2155467");
        assert!(!task.task_description.contains("Check-in notes"));
    }

    #[test]
    fn a_requester_reads_as_a_name_then_a_username() {
        assert_eq!(derek().label(), "Derek Anderson");
        let unnamed = Requester {
            name: Some(" ".into()),
            ..derek()
        };
        assert_eq!(unnamed.label(), "derek.anderson");
        let bare = Requester {
            name: None,
            email: None,
            ..derek()
        };
        assert_eq!(bare.label(), "the requesting technician");
    }

    #[test]
    fn a_username_matches_every_company_email() {
        assert_eq!(
            requester_emails(" Derek.Anderson "),
            vec![
                "derek.anderson@pclaptops.com".to_string(),
                "derek.anderson@xidax.com".to_string()
            ]
        );
        assert_eq!(
            requester_emails("derek.anderson@pclaptops.com"),
            vec!["derek.anderson@pclaptops.com".to_string()]
        );
        assert!(requester_emails("  ").is_empty());
    }
}

//! Assistant MCP tools: tasks, reminders, schedules, part routing and ticket briefs, acting for one person.

use chrono::{DateTime, NaiveTime, Utc};
use database::schema::assistant::{
    AssignedTask, PartRequest, Person, TASK_TEXT_MAX_CHARS, TASK_TITLE_MAX_CHARS, TYPE_REMINDER, TicketBrief,
    clean_line, match_person, may_assign, open_task_counts, part_due, pick_sender, post_private_note, store_from_code,
};
use database::schema::business_calendar::{CLOSE_HOUR, OPEN_HOUR};
use database::schema::odoo::parts::{self, RoutePlan};
use database::schema::service_task::{find_service_task, normalize_service_number};
use database::schema::task_schedule::{
    Every, NewTaskSchedule, Recurrence, SCHEMA_MISSING, TaskSchedule, due_for_run, local_label, parse_clock,
    parse_when, schema_applied, weekday_from_name,
};
use database::schema::{Datetime, RecordId, RecordIdExt, Store};
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ErrorCode, ErrorData},
    schemars, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::mcp_bridge::PluginToolProvider;

/// Longest FYI message `notify_user` sends, in characters.
const MESSAGE_MAX_CHARS: usize = 300;
/// Most units one part request moves.
const MAX_PART_QUANTITY: i64 = 50;

/// The person an agent session acts for.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantCaller {
    pub user: RecordId,
}

fn invalid(msg: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(msg.into(), None)
}

fn internal(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
}

fn reply(mut v: Value, now: DateTime<Utc>) -> Result<CallToolResult, ErrorData> {
    v["store_time_now"] = json!(local_label(now));
    Ok(CallToolResult::success(vec![ContentBlock::json(v).map_err(internal)?]))
}

fn store_close() -> NaiveTime {
    NaiveTime::from_hms_opt(CLOSE_HOUR, 0, 0).unwrap_or(NaiveTime::MIN)
}

fn store_open() -> NaiveTime {
    NaiveTime::from_hms_opt(OPEN_HOUR, 0, 0).unwrap_or(NaiveTime::MIN)
}

/// Task priority as stored; `Err` for an unknown name.
fn priority_name(raw: Option<&str>) -> Result<String, ErrorData> {
    let name = match raw.map(|p| p.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("normal") => "Normal",
        Some("express") => "Express",
        Some("fire") => "Fire",
        Some("rfs") => "Rfs",
        Some("qc") => "Qc",
        Some(other) => return Err(invalid(format!("priority `{other}` is not Normal, Express, Fire, Rfs or Qc"))),
    };
    Ok(name.to_string())
}

fn optional_text(field: &str, raw: Option<&str>, max: usize) -> Result<Option<String>, ErrorData> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(text) if text.chars().count() > max => Err(invalid(format!("`{field}` is over {max} characters"))),
        Some(text) => Ok(Some(text.to_string())),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Refuses when the schedule table is known to be missing; sessions that cannot tell proceed.
async fn require_schedule_table() -> Result<(), ErrorData> {
    match schema_applied().await {
        Ok(false) => Err(invalid(SCHEMA_MISSING)),
        _ => Ok(()),
    }
}

/// ISO weekday numbers from names or numbers.
fn weekday_numbers(raw: &[Value]) -> Result<Vec<i64>, ErrorData> {
    raw.iter()
        .map(|v| {
            let day = match v {
                Value::Number(n) => n.as_i64().and_then(database::schema::task_schedule::weekday_from_iso),
                Value::String(s) => weekday_from_name(s),
                _ => None,
            };
            day.map(|d| i64::from(d.number_from_monday())).ok_or_else(|| invalid(format!("`{v}` is not a weekday")))
        })
        .collect()
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CreateTaskParams {
    #[schemars(description = "Who does it: email, full or first name, or \"me\". Defaults to the requester.")]
    #[serde(default)]
    pub assignee: Option<String>,
    #[schemars(
        description = "Short imperative title, at most 120 characters, e.g. \"Call Kayleen back about her SSD\"."
    )]
    pub title: String,
    #[schemars(description = "Optional details, at most 1000 characters.")]
    #[serde(default)]
    pub description: Option<String>,
    #[schemars(
        description = "Store-local due time: \"today\", \"tomorrow 3pm\", \"friday\", \"2026-09-28 09:30\", \"in 2 hours\". A day alone means store close (19:00). Defaults to store close today."
    )]
    #[serde(default)]
    pub due: Option<String>,
    #[schemars(description = "Normal (default), Express, Fire, Rfs or Qc.")]
    #[serde(default)]
    pub priority: Option<String>,
    #[schemars(description = "Service number the task is about, when there is one.")]
    #[serde(default)]
    pub service_number: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct NotifyUserParams {
    #[schemars(description = "Who to notify: email, full or first name.")]
    pub person: String,
    #[schemars(description = "The message, at most 300 characters. It pops up on their Mastertech now.")]
    pub message: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ScheduleTaskParams {
    #[schemars(
        description = "Who gets the task each time: email, full or first name, or \"me\". Defaults to the requester."
    )]
    #[serde(default)]
    pub assignee: Option<String>,
    #[schemars(description = "Short imperative title, at most 120 characters.")]
    pub title: String,
    #[serde(default)]
    #[schemars(description = "Optional details, at most 1000 characters.")]
    pub description: Option<String>,
    #[schemars(
        description = "once (a single later reminder), day (every open day, Mon-Sat), week (on `weekdays`) or month (on `month_day`)."
    )]
    pub every: String,
    #[schemars(
        description = "For `once`: when to deliver it, e.g. \"tomorrow 3pm\", \"friday\", \"2026-09-28 09:30\", \"in 2 hours\". A day alone means 10:00."
    )]
    #[serde(default)]
    pub when: Option<String>,
    #[schemars(
        description = "Store-local time of day for day/week/month, e.g. \"10:00\" or \"3pm\". Defaults to 10:00 (store open)."
    )]
    #[serde(default)]
    pub at: Option<String>,
    #[schemars(description = "For `week`: days like [\"mon\", \"thu\"] or ISO numbers 1 (Mon) to 7 (Sun).")]
    #[serde(default)]
    pub weekdays: Vec<Value>,
    #[schemars(description = "For `month`: day of the month 1-31; short months use their last day.")]
    #[serde(default)]
    pub month_day: Option<i64>,
    #[schemars(description = "Repeat every N weeks or months (week/month only), default 1.")]
    #[serde(default)]
    pub interval: Option<i64>,
    #[schemars(description = "Hours after each delivery until the task is due. Defaults to store close that day.")]
    #[serde(default)]
    pub due_hours: Option<i64>,
    #[schemars(description = "Normal (default), Express, Fire, Rfs or Qc.")]
    #[serde(default)]
    pub priority: Option<String>,
    #[schemars(description = "Service number the task is about, when there is one.")]
    #[serde(default)]
    pub service_number: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct ListTaskSchedulesParams {
    #[schemars(description = "Whose schedules: email, full or first name. Defaults to the requester.")]
    #[serde(default)]
    pub person: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct CancelTaskScheduleParams {
    #[schemars(description = "`schedule_id` from schedule_task or list_task_schedules.")]
    pub schedule_id: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct PostTicketBriefParams {
    #[schemars(description = "Service number of the ticket.")]
    pub service_number: String,
    #[schemars(description = "What is happening with the ticket now, one sentence, at most 220 characters.")]
    pub now: String,
    #[schemars(description = "What was found, one or two sentences, at most 220 characters.")]
    pub found: String,
    #[schemars(description = "What staff should tell the customer, in plain words, at most 220 characters.")]
    pub tell_customer: String,
}

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct RoutePartParams {
    #[schemars(
        description = "Part to find, e.g. \"1TB NVMe\" or an internal reference. Every word must be in the product name, or the whole text in its code or barcode."
    )]
    #[serde(default)]
    pub part: String,
    #[schemars(description = "Units needed, default 1.")]
    #[serde(default)]
    pub quantity: Option<i64>,
    #[schemars(description = "Service ticket that needs the part.")]
    #[serde(default)]
    pub service_number: Option<String>,
    #[schemars(
        description = "Store that needs it (RIV, LTN, MUR, SAN, ORE). Defaults to the ticket technician's store, then the requester's."
    )]
    #[serde(default)]
    pub to_store: Option<String>,
    #[schemars(description = "Odoo product id from an earlier candidates list, to route that product.")]
    #[serde(default)]
    pub product_id: Option<i64>,
    #[schemars(
        description = "false (default) only reports stock and the plan; true files the send task at the chosen store."
    )]
    #[serde(default)]
    pub create_task: bool,
    #[schemars(
        description = "Person at the sending store to assign; defaults to whoever there has the fewest open tasks."
    )]
    #[serde(default)]
    pub send_to: Option<String>,
}

impl PluginToolProvider {
    /// The person these tools act for: the agent session's requester, else the signed-in user.
    async fn assistant_actor(&self) -> Result<Person, ErrorData> {
        let id = match self.assistant_caller() {
            Some(caller) => caller.user.clone(),
            None => crate::get_current_user_from_auth().map(|u| u.get_id()).ok_or_else(|| {
                invalid(
                    "these tools act for a person: sign in to Mastertech, or call from a technician's agent session",
                )
            })?,
        };
        let person = Person::load(&id)
            .await
            .map_err(internal)?
            .ok_or_else(|| invalid("the requesting user no longer exists"))?;
        if !person.active {
            return Err(invalid(format!("{} is not an active user", person.name)));
        }
        Ok(person)
    }

    async fn resolve_assignee(&self, actor: &Person, query: Option<&str>) -> Result<(Person, Vec<Person>), ErrorData> {
        let people = Person::active_all().await.map_err(internal)?;
        let target = match_person(query.unwrap_or("me"), actor, &people).map_err(invalid)?;
        may_assign(actor, &target).map_err(invalid)?;
        Ok((target, people))
    }
}

#[tool_router(router = assistant_tool_router, vis = "pub(crate)")]
impl PluginToolProvider {
    #[tool(
        name = "create_task",
        description = "Create a to-do task for someone now: yourself (\"me\") or a person in your own store; Root users can assign anyone. It appears on their task board and notifies them. For a later or repeating reminder use schedule_task; for an FYI with no task use notify_user."
    )]
    async fn create_task(&self, Parameters(p): Parameters<CreateTaskParams>) -> Result<CallToolResult, ErrorData> {
        let actor = self.assistant_actor().await?;
        let (assignee, _) = self.resolve_assignee(&actor, p.assignee.as_deref()).await?;
        let title = clean_line("title", &p.title, TASK_TITLE_MAX_CHARS).map_err(invalid)?;
        let description = optional_text("description", p.description.as_deref(), TASK_TEXT_MAX_CHARS)?;
        let now = Utc::now();
        let due = match p.due.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            Some(raw) => parse_when(raw, now, store_close()).map_err(invalid)?,
            None => due_for_run(now, None),
        };
        let task = AssignedTask {
            name: title.clone(),
            description: description.unwrap_or_default(),
            assignee: assignee.id.clone(),
            assigned_by: Some(actor.id.clone()),
            due,
            priority: priority_name(p.priority.as_deref())?,
            schedule: None,
            about_service: p.service_number.as_deref().and_then(normalize_service_number),
            part_request: None,
        };
        let id = task.create().await.map_err(internal)?;
        reply(
            json!({ "task_id": id.key_string(), "assignee": assignee.label(), "title": title, "due": local_label(due) }),
            now,
        )
    }

    #[tool(
        name = "notify_user",
        description = "Send someone a short FYI that pops up on their Mastertech now; no task is created. Same-store only unless you are Root. Use create_task when they need to do something."
    )]
    async fn notify_user(&self, Parameters(p): Parameters<NotifyUserParams>) -> Result<CallToolResult, ErrorData> {
        let actor = self.assistant_actor().await?;
        let (target, _) = self.resolve_assignee(&actor, Some(&p.person)).await?;
        let message = clean_line("message", &p.message, MESSAGE_MAX_CHARS).map_err(invalid)?;
        let text = format!("{}: {message}", actor.first_name());
        let id = database::schema::assistant::notify(&target.id, TYPE_REMINDER, &text, Some(&actor.id), None)
            .await
            .map_err(internal)?;
        reply(json!({ "notification_id": id.key_string(), "to": target.label(), "message": text }), Utc::now())
    }

    #[tool(
        name = "schedule_task",
        description = "Schedule a task for later or on repeat: every=once with `when` for a one-off reminder, day for every open day (Mon-Sat), week with `weekdays`, month with `month_day`. Each delivery creates a task and notifies the assignee. Same-store only unless you are Root. Check list_task_schedules first to avoid duplicates."
    )]
    async fn schedule_task(&self, Parameters(p): Parameters<ScheduleTaskParams>) -> Result<CallToolResult, ErrorData> {
        require_schedule_table().await?;
        let actor = self.assistant_actor().await?;
        let (assignee, _) = self.resolve_assignee(&actor, p.assignee.as_deref()).await?;
        let title = clean_line("title", &p.title, TASK_TITLE_MAX_CHARS).map_err(invalid)?;
        let description = optional_text("description", p.description.as_deref(), TASK_TEXT_MAX_CHARS)?;
        let every = Every::parse(&p.every).ok_or_else(|| invalid("`every` must be once, day, week or month"))?;
        let now = Utc::now();
        let (next_run, at, weekdays) = if every == Every::Once {
            let raw = p.when.as_deref().or(p.at.as_deref()).ok_or_else(|| invalid("`once` needs `when`"))?;
            let next = parse_when(raw, now, store_open()).map_err(invalid)?;
            let at = next.with_timezone(&database::schema::business_calendar::STORE_TZ).format("%H:%M").to_string();
            (next, at, Vec::new())
        } else {
            let at = match p.at.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
                Some(raw) => parse_clock(raw)
                    .ok_or_else(|| invalid(format!("`at` must be a time like 10:00 or 3pm, not `{raw}`")))?,
                None => store_open(),
            };
            let at = at.format("%H:%M").to_string();
            let weekdays = weekday_numbers(&p.weekdays)?;
            let rule = Recurrence::from_parts(every.as_str(), p.interval.unwrap_or(1), &weekdays, p.month_day, &at)
                .map_err(invalid)?;
            let next = rule.next_after(now, now).ok_or_else(|| invalid("that rule never fires"))?;
            (next, at, rule.weekdays.iter().map(|d| i64::from(d.number_from_monday())).collect())
        };
        let row = NewTaskSchedule {
            title: title.clone(),
            description,
            assignee: assignee.id.clone(),
            created_by: Some(actor.id.clone()),
            every: every.as_str().to_string(),
            interval: p.interval.unwrap_or(1).max(1),
            weekdays,
            month_day: if every == Every::Month { p.month_day } else { None },
            at,
            next_run: Some(Datetime::from(next_run)),
            due_hours: p.due_hours.filter(|h| (0..=720).contains(h)),
            priority: Some(priority_name(p.priority.as_deref())?),
            service_number: p.service_number.as_deref().and_then(normalize_service_number),
            origin: Some(database::schema::assistant::ASSISTANT_ORIGIN.to_string()),
        };
        let id = row.create().await.map_err(internal)?;
        let saved = TaskSchedule::get(&id).await.map_err(internal)?;
        let timing = saved.map(|s| s.describe()).unwrap_or_default();
        reply(
            json!({
                "schedule_id": id.key_string(),
                "assignee": assignee.label(),
                "title": title,
                "timing": timing,
                "next_delivery": local_label(next_run),
            }),
            now,
        )
    }

    #[tool(
        name = "list_task_schedules",
        description = "List active schedules assigned to or created by a person (default: you): id, title, timing and next delivery."
    )]
    async fn list_task_schedules(
        &self,
        Parameters(p): Parameters<ListTaskSchedulesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        require_schedule_table().await?;
        let actor = self.assistant_actor().await?;
        let (person, people) = self.resolve_assignee(&actor, p.person.as_deref()).await?;
        let rows = TaskSchedule::for_user(&person.id).await.map_err(internal)?;
        let name_of = |id: &Option<RecordId>| {
            id.as_ref().and_then(|id| people.iter().find(|p| &p.id == id)).map(Person::label).unwrap_or_default()
        };
        let schedules: Vec<Value> = rows
            .iter()
            .map(|s| {
                json!({
                    "schedule_id": s.id.key_string(),
                    "title": s.title,
                    "assignee": name_of(&Some(s.assignee.clone())),
                    "created_by": name_of(&s.created_by),
                    "timing": s.describe(),
                    "next_delivery": s.next_run.clone().map(|t| local_label(t.into_inner())),
                })
            })
            .collect();
        reply(json!({ "person": person.label(), "count": schedules.len(), "schedules": schedules }), Utc::now())
    }

    #[tool(
        name = "cancel_task_schedule",
        description = "Stop a schedule. Allowed for its creator, its assignee, or a Root user."
    )]
    async fn cancel_task_schedule(
        &self,
        Parameters(p): Parameters<CancelTaskScheduleParams>,
    ) -> Result<CallToolResult, ErrorData> {
        require_schedule_table().await?;
        let actor = self.assistant_actor().await?;
        let key = p.schedule_id.trim().trim_start_matches("task_schedule:").to_string();
        let id = RecordId::new(database::schema::task_schedule::TASK_SCHEDULE_TABLE, key);
        let schedule =
            TaskSchedule::get(&id).await.map_err(internal)?.ok_or_else(|| invalid("no schedule with that id"))?;
        let allowed =
            actor.is_root() || schedule.assignee == actor.id || schedule.created_by.as_ref() == Some(&actor.id);
        if !allowed {
            return Err(invalid("only the schedule's creator, its assignee or a Root user can cancel it"));
        }
        let changed = TaskSchedule::cancel(&id).await.map_err(internal)?;
        reply(json!({ "schedule_id": id.key_string(), "title": schedule.title, "cancelled": changed }), Utc::now())
    }

    #[tool(
        name = "post_ticket_brief",
        description = "Write the staff brief on a service ticket: three short lines (now, found, tell_customer). It is saved as a PRIVATE note that customers never see, and replaces the ticket's previous brief. Plain words, no jargon in tell_customer, each line at most 220 characters."
    )]
    async fn post_ticket_brief(
        &self,
        Parameters(p): Parameters<PostTicketBriefParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let actor = self.assistant_actor().await?;
        let sn = normalize_service_number(&p.service_number)
            .ok_or_else(|| invalid("`service_number` is not a service number"))?;
        let brief = TicketBrief::new(&p.now, &p.found, &p.tell_customer).map_err(invalid)?;
        let task = find_service_task(&sn)
            .await
            .map_err(internal)?
            .ok_or_else(|| invalid(format!("service {sn} has no task")))?;
        let note = brief.post(&task.id, &sn, &actor.id).await.map_err(internal)?;
        reply(
            json!({ "note_id": note.key_string(), "service_number": sn, "private": true, "brief": brief.render() }),
            Utc::now(),
        )
    }

    #[tool(
        name = "route_part",
        description = "Find a part in Odoo stock across the five stores for a ticket. With create_task=false (default) it reports stock and the plan: in stock here, send from the nearest store that has it, or none anywhere. Several matching products come back as candidates; call again with product_id. With create_task=true it files a send task for someone at the sending store and notes it on the ticket."
    )]
    async fn route_part(&self, Parameters(p): Parameters<RoutePartParams>) -> Result<CallToolResult, ErrorData> {
        let actor = self.assistant_actor().await?;
        let now = Utc::now();
        let quantity = p.quantity.unwrap_or(1).clamp(1, MAX_PART_QUANTITY);
        let sn = p.service_number.as_deref().and_then(normalize_service_number);
        let ticket = match &sn {
            Some(s) => find_service_task(s).await.map_err(internal)?,
            None => None,
        };
        let people = Person::active_all().await.map_err(internal)?;
        let dest = match p.to_store.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(code) => store_from_code(code)
                .ok_or_else(|| invalid(format!("unknown store `{code}`; use RIV, LTN, MUR, SAN or ORE")))?,
            None => {
                let ticket_store = match &ticket {
                    Some(t) => ticket_tech_store(&t.id).await?,
                    None => None,
                };
                ticket_store
                    .or_else(|| store_from_code(&actor.store))
                    .ok_or_else(|| invalid("say which store needs the part with `to_store`"))?
            }
        };
        let products = match p.product_id {
            Some(id) => parts::product_by_id(id).await.map_err(internal)?.into_iter().collect(),
            None if p.part.trim().is_empty() => return Err(invalid("give `part` or `product_id`")),
            None => parts::find_products(&p.part).await.map_err(internal)?,
        };
        if products.is_empty() {
            return reply(
                json!({ "found": false, "message": format!("No Odoo product matches `{}`.", p.part.trim()) }),
                now,
            );
        }
        let ids: Vec<i64> = products.iter().map(|x| x.id).collect();
        let stock = parts::store_stock(&ids).await.map_err(internal)?;
        let stock_of = |id: i64| -> Vec<(Store, f64)> {
            stock.iter().filter(|s| s.product_id == id).map(|s| (s.store, s.available)).collect()
        };
        let stock_json = |id: i64| -> Value {
            let map: serde_json::Map<String, Value> =
                stock_of(id).into_iter().map(|(s, n)| (s.as_str().to_string(), json!(n))).collect();
            Value::Object(map)
        };
        if products.len() > 1 {
            let candidates: Vec<Value> = products
                .iter()
                .map(|x| json!({ "product_id": x.id, "name": x.name, "code": x.code, "available": stock_json(x.id) }))
                .collect();
            return reply(
                json!({ "to_store": dest.as_str(), "candidates": candidates, "next": "Call again with the right product_id." }),
                now,
            );
        }
        let product = &products[0];
        let plan = parts::plan_route(dest, quantity as f64, &stock_of(product.id));
        let base = json!({
            "product_id": product.id,
            "product": product.name,
            "code": product.code,
            "quantity": quantity,
            "to_store": dest.as_str(),
            "available": stock_json(product.id),
        });
        let with = |mut v: Value, extra: Value| {
            if let (Some(obj), Some(more)) = (v.as_object_mut(), extra.as_object()) {
                obj.extend(more.clone());
            }
            v
        };
        let (from, available) = match plan {
            RoutePlan::InStock { available } => {
                return reply(
                    with(
                        base,
                        json!({ "plan": "in_stock", "message": format!("{available} available at {}; no transfer needed.", dest.as_str()) }),
                    ),
                    now,
                );
            }
            RoutePlan::NoStock => {
                return reply(
                    with(
                        base,
                        json!({ "plan": "no_stock", "message": format!("No store has {quantity} available; it has to be ordered.") }),
                    ),
                    now,
                );
            }
            RoutePlan::SendFrom { store, available } => (store, available),
        };
        if !p.create_task {
            return reply(
                with(
                    base,
                    json!({ "plan": "send_from", "from_store": from.as_str(), "message": format!("{} has {available}. Call again with create_task=true to ask them to send it.", from.as_str()) }),
                ),
                now,
            );
        }
        let sender = match p.send_to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(q) => {
                let person = match_person(q, &actor, &people).map_err(invalid)?;
                if !person.store.eq_ignore_ascii_case(from.as_str()) {
                    return Err(invalid(format!("{} is not at {}", person.label(), from.as_str())));
                }
                person
            }
            None => {
                let load = open_task_counts().await.map_err(internal)?;
                pick_sender(&people, from.as_str(), &load)
                    .ok_or_else(|| invalid(format!("nobody active at {} to send it", from.as_str())))?
            }
        };
        let for_sn = sn.as_deref().map(|s| format!(" for {s}")).unwrap_or_default();
        let head = format!("Send {quantity}× ");
        let tail = format!(" to {}{for_sn}", dest.as_str());
        let room = TASK_TITLE_MAX_CHARS.saturating_sub(head.chars().count() + tail.chars().count());
        let name = format!("{head}{}{tail}", clip(&product.name, room.max(10)));
        let code = product.code.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default();
        let description = format!(
            "Requested by {} for {}. {}{code}: {} has {available} available.",
            actor.name,
            dest.as_str(),
            product.name,
            from.as_str()
        );
        let due = part_due(now);
        let task = AssignedTask {
            name: name.clone(),
            description,
            assignee: sender.id.clone(),
            assigned_by: Some(actor.id.clone()),
            due,
            priority: "Normal".to_string(),
            schedule: None,
            about_service: sn.clone(),
            part_request: Some(PartRequest {
                part: product.name.clone(),
                quantity,
                from_store: from.as_str().to_string(),
                to_store: dest.as_str().to_string(),
                service_number: sn.clone(),
                product_code: product.code.clone(),
                product_id: Some(product.id),
            }),
        };
        let task_id = task.create().await.map_err(internal)?;
        let mut noted = false;
        if let (Some(t), Some(s)) = (&ticket, &sn) {
            let note = format!(
                "Part requested: {quantity}× {} from {} ({} to send).",
                product.name,
                from.as_str(),
                sender.name
            );
            noted = post_private_note(&t.id, s, &actor.id, &note).await.is_ok();
        }
        reply(
            with(
                base,
                json!({
                    "plan": "requested",
                    "from_store": from.as_str(),
                    "task_id": task_id.key_string(),
                    "sender": sender.label(),
                    "due": local_label(due),
                    "ticket_noted": noted,
                }),
            ),
            now,
        )
    }
}

/// The store of the technician assigned to a ticket's task.
async fn ticket_tech_store(task: &RecordId) -> Result<Option<Store>, ErrorData> {
    let stores: Vec<Option<String>> = database::db()
        .query("SELECT VALUE assignee.store FROM $id")
        .bind(("id", task.clone()))
        .await
        .map_err(internal)?
        .take(0)
        .map_err(internal)?;
    Ok(stores.into_iter().flatten().next().as_deref().and_then(store_from_code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_normalize() {
        assert_eq!(priority_name(None).unwrap(), "Normal");
        assert_eq!(priority_name(Some("FIRE")).unwrap(), "Fire");
        assert!(priority_name(Some("urgent")).is_err());
    }

    #[test]
    fn weekdays_accept_names_and_numbers() {
        let days = weekday_numbers(&[json!("mon"), json!(4), json!("Saturday")]).unwrap();
        assert_eq!(days, vec![1, 4, 6]);
        assert!(weekday_numbers(&[json!("someday")]).is_err());
    }

    #[test]
    fn clip_keeps_short_text() {
        assert_eq!(clip("SSD", 10), "SSD");
        assert_eq!(clip("abcdefghijkl", 5), "abcd…");
    }
}

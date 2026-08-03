//! Entity link resolution, validation, and cascade repointing for
//! customer / computer / connected_client / diagnostic_session graphs.

use super::{
    utilities::record_exists, ComputerData, ConnectedClient, CustomerData, RecordId,
    RecordIdExt,     COMPUTER_TABLE, CUSTOMER_TABLE,
};
use crate::db;
use serde::{Deserialize, Serialize};
use surrealdb::Error;

/// Strip SurrealQL backtick quoting from a record key (`key` → key).
pub fn strip_surreal_key_quotes(key: &str) -> String {
    let s = key.trim();
    if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn is_table_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Parse a Surreal record id from a string. Accepts `table:key`, bare `key`,
/// and SurrealQL-quoted keys (`table:`key-with:colons``).
pub fn parse_record_id(s: &str, table: &'static str) -> RecordId {
    let trimmed = s.trim();
    let key = match trimmed.split_once(':') {
        Some((prefix, rest)) if is_table_prefix(prefix) => strip_surreal_key_quotes(rest),
        _ => strip_surreal_key_quotes(trimmed),
    };
    RecordId::new(table, key)
}

/// Lookup candidates for a key: string form first, then number form for canonical integers.
fn record_id_candidates(table: &'static str, key: &str) -> Vec<RecordId> {
    let mut ids = vec![RecordId::new(table, key)];
    if let Ok(n) = key.parse::<i64>() {
        if n.to_string() == key {
            ids.push(RecordId::new(table, n));
        }
    }
    ids
}

/// Canonical computer record key for a connected Mastertech client.
pub fn canonical_computer_id(connection_string: &str) -> RecordId {
    RecordId::new(COMPUTER_TABLE, connection_string.trim())
}

/// True when the key looks like `HOSTNAME:hash9` (contains a colon).
pub fn is_canonical_computer_key(key: &str) -> bool {
    key.contains(':')
}

/// Resolve a computer id, preferring canonical `connection_string` when the
/// input is a bare hash fragment.
pub async fn resolve_computer_id(
    input: &str,
    hint_connection_string: Option<&str>,
) -> Result<RecordId, String> {
    let mut candidates: Vec<RecordId> = Vec::new();
    let parsed_key = parse_record_id(input, COMPUTER_TABLE).key_string();
    if !parsed_key.is_empty() {
        candidates.extend(record_id_candidates(COMPUTER_TABLE, &parsed_key));
    }

    // Also covers a bare hash input, whose canonical key is HOST:hash9.
    if let Some(cs) = hint_connection_string.map(str::trim).filter(|s| !s.is_empty()) {
        let canonical = canonical_computer_id(cs);
        if !candidates.iter().any(|c| c.key_string() == canonical.key_string()) {
            candidates.push(canonical);
        }
    }

    if candidates.is_empty() {
        return Err("no computer id supplied (pass computer_id or connection_string)".to_string());
    }

    for rid in &candidates {
        if matches!(record_exists(rid.clone()).await, Ok(Some(true))) {
            return Ok(rid.clone());
        }
    }

    Err(format!(
        "no computer row exists for {input:?}; searched keys {:?}. Mint the canonical row with \
         link_connected_client {{ connection_string, customer_id }}.",
        candidates.iter().map(RecordIdExt::key_string).collect::<Vec<_>>()
    ))
}

/// Resolve an id param to an existing row, trying the string key then the number key.
pub async fn resolve_record_id(input: &str, table: &'static str) -> Result<RecordId, String> {
    let key = parse_record_id(input, table).key_string();
    for rid in record_id_candidates(table, &key) {
        if matches!(record_exists(rid.clone()).await, Ok(Some(true))) {
            return Ok(rid);
        }
    }
    Err(format!("no {table} row found for {input:?}"))
}

pub async fn resolve_customer_id(input: &str) -> Result<RecordId, String> {
    resolve_record_id(input, CUSTOMER_TABLE).await
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectedClientGraph {
    pub client: Option<ConnectedClient>,
    pub computer: Option<ComputerData>,
    pub customer: Option<CustomerData>,
}

pub async fn load_connected_client_graph(
    connection_string: &str,
) -> Result<ConnectedClientGraph, Error> {
    let cs = connection_string.trim();
    let clients: Vec<ConnectedClient> = db()
        .query(
            "SELECT * FROM connected_client WHERE connection_string == $cs LIMIT 1",
        )
        .bind(("cs", cs.to_string()))
        .await?
        .take(0)?;

    let Some(client) = clients.into_iter().next() else {
        return Ok(ConnectedClientGraph::default());
    };

    let computer = if let Some(ref cid) = client.computer {
        db()
            .select::<Option<ComputerData>>(cid.clone())
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let customer = if let Some(ref cust_id) = client.customer {
        db()
            .select::<Option<CustomerData>>(cust_id.clone())
            .await
            .ok()
            .flatten()
    } else if let Some(ref comp) = computer {
        if let Some(ref cust_id) = comp.customer {
            db()
                .select::<Option<CustomerData>>(cust_id.clone())
                .await
                .ok()
                .flatten()
        } else {
            None
        }
    } else {
        None
    };

    Ok(ConnectedClientGraph {
        client: Some(client),
        computer,
        customer,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkValidationIssue {
    MissingCustomer,
    MissingComputer,
    CustomerNotFound,
    ComputerNotFound,
    ComputerKeyNotCanonical { expected_connection_string: String, actual_key: String },
    CustomerComputerMismatch {
        customer_id: String,
        computer_customer_id: String,
    },
    ConnectedClientComputerMismatch {
        connection_string: String,
        client_computer: String,
        requested_computer: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkBundle {
    pub connection_string: Option<String>,
    /// None resolves from `connected_client.customer`, else `computer.customer`.
    pub customer_id: Option<RecordId>,
    /// None resolves to the canonical `computer:HOST:hash9`.
    pub computer_id: Option<RecordId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinkValidationResult {
    pub ok: bool,
    pub issues: Vec<LinkValidationIssue>,
    pub resolved_customer_id: Option<RecordId>,
    pub resolved_computer_id: Option<RecordId>,
}

pub async fn validate_link_bundle(bundle: &LinkBundle) -> LinkValidationResult {
    let mut issues = Vec::new();
    let mut resolved_customer_id = None;
    let mut resolved_computer_id = None;

    let cs = bundle
        .connection_string
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let graph = match cs {
        Some(cs) => load_connected_client_graph(cs).await.ok(),
        None => None,
    };

    let requested_customer = bundle.customer_id.clone().or_else(|| {
        let g = graph.as_ref()?;
        g.client
            .as_ref()
            .and_then(|c| c.customer.clone())
            .or_else(|| g.computer.as_ref().and_then(|c| c.customer.clone()))
    });

    match requested_customer {
        Some(cust) => match resolve_customer_id(&cust.key_string()).await {
            Ok(rid) => resolved_customer_id = Some(rid),
            Err(_) => issues.push(LinkValidationIssue::CustomerNotFound),
        },
        None => issues.push(LinkValidationIssue::MissingCustomer),
    }

    let computer_input = bundle
        .computer_id
        .as_ref()
        .map(RecordIdExt::key_string)
        .unwrap_or_default();
    match resolve_computer_id(&computer_input, cs).await {
        Ok(rid) => {
            resolved_computer_id = Some(rid.clone());
            if let Some(cs) = cs {
                if rid.key_string() != cs || !is_canonical_computer_key(cs) {
                    issues.push(LinkValidationIssue::ComputerKeyNotCanonical {
                        expected_connection_string: cs.to_string(),
                        actual_key: rid.key_string(),
                    });
                }
            }
        }
        Err(_) => issues.push(LinkValidationIssue::ComputerNotFound),
    }

    if let (Some(cust), Some(comp_id)) = (&resolved_customer_id, &resolved_computer_id) {
        if let Ok(Some(comp)) = db().select::<Option<ComputerData>>(comp_id.clone()).await {
            if let Some(comp_cust) = &comp.customer {
                if comp_cust.key_string() != cust.key_string() {
                    issues.push(LinkValidationIssue::CustomerComputerMismatch {
                        customer_id: cust.key_string(),
                        computer_customer_id: comp_cust.key_string(),
                    });
                }
            } else {
                issues.push(LinkValidationIssue::MissingCustomer);
            }
        } else {
            issues.push(LinkValidationIssue::MissingComputer);
        }
    }

    if let (Some(cs), Some(client)) = (cs, graph.as_ref().and_then(|g| g.client.as_ref())) {
        match (&client.computer, &resolved_computer_id) {
            (Some(client_comp), Some(resolved))
                if client_comp.key_string() != resolved.key_string() =>
            {
                issues.push(LinkValidationIssue::ConnectedClientComputerMismatch {
                    connection_string: cs.to_string(),
                    client_computer: client_comp.key_string(),
                    requested_computer: resolved.key_string(),
                });
            }
            (None, Some(_)) => issues.push(LinkValidationIssue::MissingComputer),
            _ => {}
        }
    }

    let ok = issues.is_empty();
    LinkValidationResult {
        ok,
        issues,
        resolved_customer_id,
        resolved_computer_id,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CascadeReport {
    pub connected_clients: u64,
    pub diagnostic_sessions: u64,
    pub service_orders: u64,
    pub computers_customer_field: u64,
}

async fn count_updated(query: &str, old: &RecordId, new: &RecordId) -> Result<u64, Error> {
    let rows: Vec<RecordId> = db()
        .query(query)
        .bind(("old", old.clone()))
        .bind(("new", new.clone()))
        .await?
        .take(0)?;
    Ok(rows.len() as u64)
}

pub async fn cascade_repoint_computer(
    old_id: &RecordId,
    new_id: &RecordId,
) -> Result<CascadeReport, Error> {
    if old_id.key_string() == new_id.key_string() {
        return Ok(CascadeReport::default());
    }

    let mut report = CascadeReport::default();

    report.connected_clients = count_updated(
        "UPDATE connected_client SET computer = $new WHERE computer == $old RETURN id",
        old_id,
        new_id,
    )
    .await?;

    report.diagnostic_sessions = count_updated(
        "UPDATE diagnostic_session SET computer_id = $new WHERE computer_id == $old RETURN id",
        old_id,
        new_id,
    )
    .await?;

    report.service_orders = count_updated(
        "UPDATE service_order SET computer = $new WHERE computer == $old RETURN id",
        old_id,
        new_id,
    )
    .await?;

    // Tasks reference computer indirectly via service_ticket; update tickets above covers SO.

    Ok(report)
}

pub async fn cascade_repoint_customer(
    old_id: &RecordId,
    new_id: &RecordId,
) -> Result<CascadeReport, Error> {
    if old_id.key_string() == new_id.key_string() {
        return Ok(CascadeReport::default());
    }

    let mut report = CascadeReport::default();

    let _: Vec<RecordId> = db()
        .query("UPDATE connected_client SET customer = $new WHERE customer == $old RETURN id")
        .bind(("old", old_id.clone()))
        .bind(("new", new_id.clone()))
        .await?
        .take(0)?;

    report.computers_customer_field = count_updated(
        "UPDATE computer SET customer = $new WHERE customer == $old RETURN id",
        old_id,
        new_id,
    )
    .await?;

    report.diagnostic_sessions = count_updated(
        "UPDATE diagnostic_session SET customer_id = $new WHERE customer_id == $old RETURN id",
        old_id,
        new_id,
    )
    .await?;

    report.service_orders = count_updated(
        "UPDATE service_order SET customer = $new WHERE customer == $old RETURN id",
        old_id,
        new_id,
    )
    .await?;

    Ok(report)
}

/// Delete a computer row after cascade repoint when safe.
pub async fn delete_computer_if_unreferenced(id: &RecordId) -> Result<bool, Error> {
    let still_linked: bool = db()
        .query(
            "RETURN (
                (SELECT count() FROM connected_client WHERE computer == $id)[0].count > 0
                OR (SELECT count() FROM diagnostic_session WHERE computer_id == $id)[0].count > 0
                OR (SELECT count() FROM service_order WHERE computer == $id)[0].count > 0
            )",
        )
        .bind(("id", id.clone()))
        .await?
        .take::<Option<bool>>(0)?
        .unwrap_or(false);

    if still_linked {
        return Ok(false);
    }

    let _: Option<ComputerData> = db().delete(id.clone()).await?;
    Ok(true)
}

/// Heuristic: Presta-only placeholder with no real hardware identity.
pub fn is_placeholder_computer(computer: &ComputerData) -> bool {
    let model = computer
        .device_model
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    let has_hardware = !computer.cpu.is_empty()
        || computer
            .device_serial
            .as_ref()
            .is_some_and(|s| !s.is_empty())
        || !computer.hostname.is_empty()
        || computer
            .device_mfg
            .as_ref()
            .is_some_and(|s| !s.is_empty() && s != "PC Laptops PCL");

    if has_hardware {
        return false;
    }

    model.contains("diagnosis") || model.contains("diagnostic") || computer.hostname.is_empty()
}

/// Whether a computer row is suitable to persist as part of task creation.
pub fn computer_has_minimal_hardware(computer: &ComputerData) -> bool {
    !computer.hostname.is_empty()
        && (!computer.cpu.is_empty()
            || computer
                .device_serial
                .as_ref()
                .is_some_and(|s| !s.is_empty()))
}

/// Repair links for a connected client: ensure canonical computer exists,
/// repoint placeholders, align diagnostic sessions.
pub async fn repair_connection_links(
    connection_string: &str,
) -> Result<serde_json::Value, anyhow::Error> {
    let graph = load_connected_client_graph(connection_string)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let canonical = canonical_computer_id(connection_string);

    let mut report = serde_json::json!({
        "connection_string": connection_string,
        "canonical_computer": canonical.key_string(),
        "actions": [],
    });

    let client = graph
        .client
        .ok_or_else(|| anyhow::anyhow!("connected_client not found for {connection_string}"))?;

    if let Some(ref old) = client.computer {
        if old.key_string() != canonical.key_string() {
            let cascade = cascade_repoint_computer(old, &canonical)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            report["actions"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "repoint_computer": { "from": old.key_string(), "cascade": cascade }
                }));
            let _ = delete_computer_if_unreferenced(old).await;
        }
    }

    db()
        .query("UPDATE connected_client SET computer = $cid WHERE connection_string == $cs RETURN AFTER")
        .bind(("cid", canonical.clone()))
        .bind(("cs", connection_string.to_string()))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .take::<Option<ConnectedClient>>(0)?;

    // Repoint diagnostic sessions on this connection_string with wrong computer_id
    let sessions: Vec<RecordId> = db()
        .query(
            "UPDATE diagnostic_session SET computer_id = $cid \
             WHERE connection_string == $cs AND computer_id != $cid RETURN id",
        )
        .bind(("cid", canonical.clone()))
        .bind(("cs", connection_string.to_string()))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .take(0)?;

    report["diagnostic_sessions_updated"] = serde_json::json!(sessions.len());

    if let Some(cust) = client.customer.clone() {
        if matches!(record_exists(cust.clone()).await, Ok(Some(true))) {
            db()
                .query("UPDATE $cid SET customer = $cust RETURN AFTER")
                .bind(("cid", canonical.clone()))
                .bind(("cust", cust))
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .take::<Option<ComputerData>>(0)?;
        } else {
            report["dangling_customer_not_propagated"] =
                serde_json::json!(cust.key_string());
        }
    }

    Ok(report)
}

/// Link a connected client to a customer and its canonical computer record,
/// creating the `computer:HOST:hash9` row when missing. Built for the
/// hardware-swap case where a machine reconnects under a new persistent
/// client id with null customer/computer (which `repair_connection_links`
/// can't fix — it only repoints existing links and never mints the row).
///
/// Upserts the computer (sets customer + hostname only — never clobbers
/// existing specs), then links `connected_client.customer/computer` and,
/// when supplied, `friendly_name`. Component specs (CPU/GPU/RAM) are left to
/// the client's own check-in to populate, since on a part swap they differ
/// from any record carried forward.
pub async fn link_connected_client_record(
    connection_string: &str,
    customer_id: &str,
    friendly_name: Option<&str>,
) -> Result<serde_json::Value, anyhow::Error> {
    let cs = connection_string.trim();
    let customer = resolve_customer_id(customer_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let canonical = canonical_computer_id(cs);
    let hostname = cs.split(':').next().unwrap_or(cs).to_string();

    let graph = load_connected_client_graph(cs)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if graph.client.is_none() {
        anyhow::bail!("connected_client not found for {cs}");
    }
    let computer_existed = graph.computer.is_some();

    db()
        .query("UPSERT $cid SET customer = $cust, hostname = $host")
        .bind(("cid", canonical.clone()))
        .bind(("cust", customer.clone()))
        .bind(("host", hostname))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .take::<Option<ComputerData>>(0)?;

    db()
        .query(
            "UPDATE connected_client \
             SET customer = $cust, computer = $cid, friendly_name = $fname ?? friendly_name \
             WHERE connection_string == $cs RETURN AFTER",
        )
        .bind(("cust", customer.clone()))
        .bind(("cid", canonical.clone()))
        .bind(("fname", friendly_name.map(|s| s.to_string())))
        .bind(("cs", cs.to_string()))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .take::<Option<ConnectedClient>>(0)?;

    Ok(serde_json::json!({
        "connection_string": cs,
        "customer": customer.key_string(),
        "computer": canonical.key_string(),
        "computer_created": !computer_existed,
        "friendly_name": friendly_name,
        "linked": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{TASK_TABLE, TICKET_TABLE};
    use surrealdb::types::RecordIdKey;

    #[test]
    fn strip_surreal_key_quotes_round_trip() {
        assert_eq!(strip_surreal_key_quotes("`197987`"), "197987");
        assert_eq!(
            strip_surreal_key_quotes("`DESKTOP-HQAF13L:b57a7e8f9`"),
            "DESKTOP-HQAF13L:b57a7e8f9"
        );
        assert_eq!(strip_surreal_key_quotes("plain"), "plain");
    }

    #[test]
    fn parse_record_id_accepts_surreal_quoted_keys() {
        assert_eq!(
            parse_record_id("customer:`197987`", CUSTOMER_TABLE).key_string(),
            "197987"
        );
        assert_eq!(
            parse_record_id("computer:`DESKTOP-HQAF13L:b57a7e8f9`", COMPUTER_TABLE).key_string(),
            "DESKTOP-HQAF13L:b57a7e8f9"
        );
        assert_eq!(
            parse_record_id("service_order:`52918345`", TICKET_TABLE).key_string(),
            "52918345"
        );
    }

    #[test]
    fn record_id_candidates_prefers_string_then_number() {
        let numeric = record_id_candidates(CUSTOMER_TABLE, "51944");
        assert_eq!(numeric.len(), 2);
        assert!(matches!(numeric[0].key, RecordIdKey::String(_)));
        assert!(matches!(numeric[1].key, RecordIdKey::Number(51944)));

        // Leading zeros and non-integers have no number form to fall back to.
        assert_eq!(record_id_candidates(CUSTOMER_TABLE, "0051944").len(), 1);
        assert_eq!(
            record_id_candidates(COMPUTER_TABLE, "DESKTOP-36JF8OV:e765bf42b").len(),
            1
        );
    }

    #[test]
    fn parse_record_id_plain_forms_unchanged() {
        assert_eq!(
            parse_record_id("computer:DESKTOP-HQAF13L:b57a7e8f9", COMPUTER_TABLE).key_string(),
            "DESKTOP-HQAF13L:b57a7e8f9"
        );
        assert_eq!(parse_record_id("197987", CUSTOMER_TABLE).key_string(), "197987");
        assert_eq!(
            parse_record_id("DESKTOP-HQAF13L:b57a7e8f9", COMPUTER_TABLE).key_string(),
            "DESKTOP-HQAF13L:b57a7e8f9"
        );
        assert_eq!(
            parse_record_id("task:cliyh7tklg89djkq0ejd", TASK_TABLE).key_string(),
            "cliyh7tklg89djkq0ejd"
        );
    }
}

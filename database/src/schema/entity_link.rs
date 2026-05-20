//! Entity link resolution, validation, and cascade repointing for
//! customer / computer / connected_client / diagnostic_session graphs.

use super::{
    utilities::record_exists, ComputerData, ConnectedClient, CustomerData, RecordId,
    RecordIdExt,     COMPUTER_TABLE, CUSTOMER_TABLE,
};
use crate::DATABASE;
use serde::{Deserialize, Serialize};
use surrealdb::Error;

/// Parse a Surreal record id from a string. Accepts `table:key` or bare `key`.
/// Only strips the first colon when the prefix looks like a table name.
pub fn parse_record_id(s: &str, table: &'static str) -> RecordId {
    let trimmed = s.trim();
    let key = match trimmed.split_once(':') {
        Some((prefix, rest))
            if !prefix.is_empty()
                && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            rest.to_string()
        }
        _ => trimmed.to_string(),
    };
    RecordId::new(table, key)
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
    let mut candidates = Vec::new();
    let parsed = parse_record_id(input, COMPUTER_TABLE);
    candidates.push(parsed.clone());

    if let Some(cs) = hint_connection_string.filter(|s| !s.is_empty()) {
        let canonical = canonical_computer_id(cs);
        if !candidates.iter().any(|c| c.key_string() == canonical.key_string()) {
            candidates.push(canonical);
        }
        // Bare hash suffix match: DESKTOP-X:abc from input "abc" or "computer:abc"
        if !parsed.key_string().contains(':') {
            if cs.ends_with(&format!(":{}", parsed.key_string())) {
                // already have canonical
            } else if let Some((_host, hash)) = cs.rsplit_once(':') {
                if hash == parsed.key_string() {
                    candidates.push(canonical_computer_id(cs));
                }
            }
        }
    }

    for rid in &candidates {
        if matches!(record_exists(rid.clone()).await, Ok(Some(true))) {
            return Ok(rid.clone());
        }
    }

    Err(format!(
        "no computer row found for {input:?} (tried {:?})",
        candidates
            .iter()
            .map(|c| c.key_string())
            .collect::<Vec<_>>()
    ))
}

pub async fn resolve_customer_id(input: &str) -> Result<RecordId, String> {
    let rid = parse_record_id(input, CUSTOMER_TABLE);
    match record_exists(rid.clone()).await {
        Ok(Some(true)) => Ok(rid),
        _ => Err(format!("no customer row found for {input:?}")),
    }
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
    let clients: Vec<ConnectedClient> = DATABASE
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
        DATABASE
            .select::<Option<ComputerData>>(cid.clone())
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let customer = if let Some(ref cust_id) = client.customer {
        DATABASE
            .select::<Option<CustomerData>>(cust_id.clone())
            .await
            .ok()
            .flatten()
    } else if let Some(ref comp) = computer {
        if let Some(ref cust_id) = comp.customer {
            DATABASE
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
    pub customer_id: RecordId,
    pub computer_id: RecordId,
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

    match record_exists(bundle.customer_id.clone()).await {
        Ok(Some(true)) => resolved_customer_id = Some(bundle.customer_id.clone()),
        _ => issues.push(LinkValidationIssue::CustomerNotFound),
    }

    match resolve_computer_id(
        &bundle.computer_id.key_string(),
        bundle.connection_string.as_deref(),
    )
    .await
    {
        Ok(rid) => {
            resolved_computer_id = Some(rid.clone());
            if let Some(cs) = bundle.connection_string.as_deref() {
                if !cs.is_empty() && rid.key_string() != cs.trim() {
                    issues.push(LinkValidationIssue::ComputerKeyNotCanonical {
                        expected_connection_string: cs.trim().to_string(),
                        actual_key: rid.key_string(),
                    });
                }
            }
            if !is_canonical_computer_key(&rid.key_string()) {
                if let Some(cs) = bundle.connection_string.as_deref().filter(|s| !s.is_empty()) {
                    issues.push(LinkValidationIssue::ComputerKeyNotCanonical {
                        expected_connection_string: cs.to_string(),
                        actual_key: rid.key_string(),
                    });
                }
            }
            match record_exists(rid.clone()).await {
                Ok(Some(true)) => {}
                _ => issues.push(LinkValidationIssue::ComputerNotFound),
            }
        }
        Err(_) => issues.push(LinkValidationIssue::ComputerNotFound),
    }

    if let (Some(cust), Some(comp_id)) = (&resolved_customer_id, &resolved_computer_id) {
        if let Ok(Some(comp)) = DATABASE.select::<Option<ComputerData>>(comp_id.clone()).await {
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

    if let Some(cs) = bundle.connection_string.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(graph) = load_connected_client_graph(cs).await {
            if let Some(client) = graph.client {
                if let Some(ref client_comp) = client.computer {
                    if let Some(ref resolved) = resolved_computer_id {
                        if client_comp.key_string() != resolved.key_string() {
                            issues.push(LinkValidationIssue::ConnectedClientComputerMismatch {
                                connection_string: cs.to_string(),
                                client_computer: client_comp.key_string(),
                                requested_computer: resolved.key_string(),
                            });
                        }
                    }
                } else if resolved_computer_id.is_some() {
                    issues.push(LinkValidationIssue::MissingComputer);
                }
            }
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
    let rows: Vec<RecordId> = DATABASE
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

    let _: Vec<RecordId> = DATABASE
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
    let still_linked: bool = DATABASE
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

    let _: Option<ComputerData> = DATABASE.delete(id.clone()).await?;
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

    DATABASE
        .query("UPDATE connected_client SET computer = $cid WHERE connection_string == $cs RETURN AFTER")
        .bind(("cid", canonical.clone()))
        .bind(("cs", connection_string.to_string()))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .take::<Option<ConnectedClient>>(0)?;

    // Repoint diagnostic sessions on this connection_string with wrong computer_id
    let sessions: Vec<RecordId> = DATABASE
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
        DATABASE
            .query("UPDATE $cid SET customer = $cust RETURN AFTER")
            .bind(("cid", canonical.clone()))
            .bind(("cust", cust))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .take::<Option<ComputerData>>(0)?;
    }

    Ok(report)
}

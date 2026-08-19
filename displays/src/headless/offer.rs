//! Resolves a newly connected machine to its open service order and offers the
//! tech an AI-assisted diagnostic.
//!
//! Writes an `assist_offer` row rather than sending a command, so the offer
//! survives a client restart and the wire shape stays untouched. The client
//! polls its own offers; a tech answering one is what creates an assist request.

use database::schema::RecordIdExt;

/// A machine is not re-offered inside this window, connected or not.
const REOFFER_QUIET_HOURS: u32 = 12;
/// Service orders older than this are history, not the job on the bench.
const ORDER_LOOKBACK_DAYS: u32 = 45;

struct ResolvedOrder {
    id: database::schema::RecordId,
    service_number: String,
    customer_name: Option<String>,
    device: Option<String>,
    checkin_notes: Option<String>,
}

/// Newest open service order for the machine, if one looks current.
async fn resolve_order(computer_key: &str) -> Option<ResolvedOrder> {
    let computer = database::schema::RecordId::new(
        database::schema::COMPUTER_TABLE,
        computer_key.trim(),
    );
    let sql = format!(
        "SELECT id, service_number, created_at, customer.name AS customer_name, \
         checkin_notes, computer.device_mfg AS device_mfg, computer.device_model AS device_model FROM service_order \
         WHERE computer = $computer AND created_at > time::now() - {ORDER_LOOKBACK_DAYS}d \
         ORDER BY created_at DESC LIMIT 1"
    );
    let rows: Vec<serde_json::Value> = database::db()
        .query(sql)
        .bind(("computer", computer))
        .await
        .ok()?
        .take(0)
        .ok()?;
    let row = rows.first()?;
    let service_number = row.get("service_number")?.as_str()?.to_string();
    if service_number.trim().is_empty() {
        return None;
    }
    let id = row.get("id").map(|v| match v {
        serde_json::Value::String(s) => database::schema::RecordId::new(
            database::schema::TICKET_TABLE,
            s.trim_start_matches("service_order:").trim_matches('`'),
        ),
        other => database::schema::RecordId::new(
            database::schema::TICKET_TABLE,
            other.to_string().trim_matches('"').to_string(),
        ),
    })?;
    Some(ResolvedOrder {
        id,
        service_number,
        customer_name: row.get("customer_name").and_then(|v| v.as_str()).map(str::to_string),
        device: {
            let part = |k: &str| {
                row.get(k).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string()
            };
            let joined = format!("{} {}", part("device_mfg"), part("device_model")).trim().to_string();
            (!joined.is_empty()).then_some(joined)
        },
        checkin_notes: row
            .get("checkin_notes")
            .and_then(|v| v.as_str())
            .map(|n| n.chars().take(160).collect()),
    })
}

/// True when this machine was already offered or already asked recently.
async fn recently_handled(connection_string: &str) -> bool {
    let sql = format!(
        "SELECT count() FROM assist_offer WHERE connection_string = $cs \
         AND created_at > time::now() - {REOFFER_QUIET_HOURS}h GROUP ALL; \
         SELECT count() FROM assist_request WHERE connection_string = $cs \
         AND created_at > time::now() - {REOFFER_QUIET_HOURS}h GROUP ALL"
    );
    let Ok(mut res) = database::db().query(sql).bind(("cs", connection_string.to_string())).await
    else {
        // Unknown is treated as handled; a missed offer beats a duplicate one.
        return true;
    };
    let count = |rows: Vec<serde_json::Value>| -> i64 {
        rows.first()
            .and_then(|r| r.get("count"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
    };
    count(res.take(0).unwrap_or_default()) > 0 || count(res.take(1).unwrap_or_default()) > 0
}

/// Offers assistance for a freshly opened session; silent when nothing matches.
pub async fn offer_for(connection_string: &str, computer: Option<&database::schema::RecordId>) {
    let Some(computer) = computer else { return };
    if recently_handled(connection_string).await {
        return;
    }
    let Some(order) = resolve_order(&computer.key_string()).await else {
        log::debug!("offer: no current service order for {connection_string}");
        return;
    };
    let res = database::db()
        .query(
            "CREATE assist_offer CONTENT { connection_string: $cs, service_number: $sn, \
             service_order: $order, customer_name: $customer, device: $device, \
             checkin_notes: $notes, reason: $reason, status: 'offered' }",
        )
        .bind(("cs", connection_string.to_string()))
        .bind(("sn", order.service_number.clone()))
        .bind(("order", order.id))
        .bind(("customer", order.customer_name))
        .bind(("device", order.device))
        .bind(("notes", order.checkin_notes))
        .bind((
            "reason",
            format!("Machine matched service #{} by computer record", order.service_number),
        ))
        .await;
    match res {
        Ok(_) => log::info!(
            "offer: {connection_string} -> service #{}",
            order.service_number
        ),
        Err(e) => log::warn!("offer: write failed for {connection_string}: {e}"),
    }
}

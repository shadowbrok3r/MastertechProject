//! Materializes the customer and service_order records for an order that nobody
//! has opened a task for yet.
//!
//! The shelf sweep already pulls the full PrestaShop order to score a candidate,
//! so the records may as well exist: most of those orders get a task anyway.
//! Mapping goes through `apply_prestashop_payload`, so the ids match exactly what
//! the task-creation path would produce and a later task adopts these rows.
//!
//! No computer row is written. The canonical computer key is derived from the
//! live machine (`hostname:hash9`), which order data cannot know, so minting one
//! here would create a second row for a machine that is not even plugged in yet.

use log::{info, warn};

use super::task_creation::{
    apply_prestashop_payload, fetch_prestashop_order, EntityDraft, OrderLookup, PrestaMapMode,
    PrestaMapOptions,
};
use super::{CustomerData, RecordId, TICKET_TABLE};
use crate::db;

/// What `ensure_order_records` did, so a caller can report it without guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeOutcome {
    pub service_number: String,
    pub customer: String,
    pub service_order: String,
    /// True when the service_order row already existed.
    pub reused_order: bool,
    /// True when the customer resolved to a row other than the mapped id.
    pub reused_customer: bool,
}

/// Resolves an existing customer by the unique-indexed `cust_code`/`email`.
async fn find_customer_by_identity(customer: &CustomerData) -> Option<RecordId> {
    let mut res = db()
        .query(
            "SELECT VALUE id FROM customer \
             WHERE (cust_code != NONE AND cust_code = $cust_code) \
                OR (email != NONE AND email = $email) LIMIT 1",
        )
        .bind(("cust_code", customer.cust_code.clone()))
        .bind(("email", customer.email.clone()))
        .await
        .ok()?;
    res.take::<Vec<RecordId>>(0).ok()?.into_iter().next()
}

/// Id of the service_order already filed under a service number.
async fn order_id_for(service_number: &str) -> anyhow::Result<Option<RecordId>> {
    let mut res = db()
        .query("SELECT VALUE id FROM service_order WHERE service_number == $sn LIMIT 1")
        .bind(("sn", service_number.to_string()))
        .await?;
    Ok(res.take::<Vec<RecordId>>(0)?.into_iter().next())
}

/// An existing customer row for a mapped customer: its own id, else a non-blank cust_code or email.
async fn adoptable_customer(customer: &CustomerData) -> anyhow::Result<Option<RecordId>> {
    let by_id: Option<super::Record> = db().select(customer.id.clone()).await?;
    if let Some(row) = by_id {
        return Ok(Some(row.id));
    }
    let mut res = db()
        .query(
            "SELECT VALUE id FROM customer \
             WHERE ($cust_code != '' AND cust_code == $cust_code) \
                OR ($email != '' AND email == $email) LIMIT 1",
        )
        .bind(("cust_code", customer.cust_code.trim().to_string()))
        .bind(("email", customer.email.trim().to_string()))
        .await?;
    Ok(res.take::<Vec<RecordId>>(0)?.into_iter().next())
}

/// Creates a missing order's customer and service_order from a read-only PrestaShop pull; `None` when the order exists.
pub async fn materialize_order_read_only(
    service_number: &str,
    computer: Option<&RecordId>,
) -> anyhow::Result<Option<IntakeOutcome>> {
    let service_number = service_number.trim();
    if service_number.is_empty() {
        anyhow::bail!("service number required");
    }
    if order_id_for(service_number).await?.is_some() {
        return Ok(None);
    }
    let payload = super::utilities::get_prestashop_payload_read_only(service_number).await?;
    let mut draft = EntityDraft::default();
    apply_prestashop_payload(
        &payload,
        &mut draft,
        &PrestaMapOptions { mode: PrestaMapMode::Audit, ..Default::default() },
    );
    if draft.customer.name.trim().is_empty() && draft.customer.email.trim().is_empty() {
        anyhow::bail!("order {service_number} carries no customer identity");
    }

    let (customer_id, reused_customer) = match adoptable_customer(&draft.customer).await? {
        Some(existing) => (existing, true),
        None => match db()
            .create::<Option<super::Record>>(draft.customer.id.clone())
            .content(draft.customer.clone())
            .await
        {
            Ok(record) => (record.map(|r| r.id).unwrap_or_else(|| draft.customer.id.clone()), false),
            Err(e) => match adoptable_customer(&draft.customer).await? {
                Some(existing) => (existing, true),
                None => anyhow::bail!("could not write customer for #{service_number}: {e}"),
            },
        },
    };

    let new_order = RecordId::new(TICKET_TABLE, service_number.to_string());
    draft.ticket.id = new_order.clone();
    draft.ticket.customer = Some(customer_id.clone());
    draft.ticket.computer = computer.cloned();
    let (order_id, reused_order) = match db()
        .create::<Option<super::Record>>(new_order.clone())
        .content(draft.ticket.clone())
        .await
    {
        Ok(_) => (new_order, false),
        Err(e) => match order_id_for(service_number).await? {
            Some(existing) => (existing, true),
            None => anyhow::bail!("could not write service_order for #{service_number}: {e}"),
        },
    };
    info!("materialize_order_read_only: #{service_number} customer {customer_id:?}, reused order {reused_order}");

    use super::RecordIdExt;
    Ok(Some(IntakeOutcome {
        service_number: service_number.to_string(),
        customer: customer_id.key_string(),
        service_order: order_id.key_string(),
        reused_order,
        reused_customer,
    }))
}

/// Creates or adopts the customer and service_order for a service number.
pub async fn ensure_order_records(service_number: &str) -> anyhow::Result<IntakeOutcome> {
    let service_number = service_number.trim();
    if service_number.is_empty() {
        anyhow::bail!("service number required");
    }
    let payload =
        fetch_prestashop_order(OrderLookup::ServiceNumber(service_number.to_string())).await?;

    let mut draft = EntityDraft::default();
    apply_prestashop_payload(
        &payload,
        &mut draft,
        &PrestaMapOptions { mode: PrestaMapMode::Audit, ..Default::default() },
    );

    if draft.customer.name.trim().is_empty() && draft.customer.email.trim().is_empty() {
        anyhow::bail!("order {service_number} carries no customer identity");
    }

    let mapped_customer = draft.customer.id.clone();
    let mut reused_customer = false;
    let customer_id = match db()
        .upsert::<Option<super::Record>>(mapped_customer.clone())
        .content(draft.customer.clone())
        .await
    {
        Ok(record) => record.map(|r| r.id).unwrap_or(mapped_customer.clone()),
        // A unique cust_code/email under a different id lands here.
        Err(e) => match find_customer_by_identity(&draft.customer).await {
            Some(existing) => {
                reused_customer = true;
                info!("ensure_order_records: reusing customer {existing:?} for #{service_number}");
                existing
            },
            None => anyhow::bail!("could not write customer for #{service_number}: {e}"),
        },
    };

    let existing_order: Option<RecordId> = match db()
        .query("SELECT VALUE id FROM service_order WHERE service_number == $sn LIMIT 1")
        .bind(("sn", service_number.to_string()))
        .await
    {
        Ok(mut res) => res.take(0).unwrap_or_default(),
        Err(e) => {
            warn!("ensure_order_records: order lookup failed for #{service_number}: {e:?}");
            None
        },
    };
    let reused_order = existing_order.is_some();
    let order_id = existing_order
        .unwrap_or_else(|| RecordId::new(TICKET_TABLE, service_number.to_string()));

    draft.ticket.id = order_id.clone();
    draft.ticket.customer = Some(customer_id.clone());
    // Left unset on purpose: the machine is not connected, so it has no canonical key.
    draft.ticket.computer = None;

    db().upsert::<Option<super::Record>>(order_id.clone())
        .content(draft.ticket.clone())
        .await
        .map_err(|e| anyhow::anyhow!("could not write service_order for #{service_number}: {e}"))?;

    use super::RecordIdExt;
    Ok(IntakeOutcome {
        service_number: service_number.to_string(),
        customer: customer_id.key_string(),
        service_order: order_id.key_string(),
        reused_order,
        reused_customer,
    })
}

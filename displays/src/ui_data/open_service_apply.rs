//! Stage 5: persist an operator-confirmed open-service bind.

use crate::modals::OpenServiceConfirmApply;
use database::schema::entity_link::{canonical_computer_id, parse_record_id};
use database::schema::service_match::PrestaSpecsSnapshot;
use database::schema::{
    ComputerData, CustomerData, RecordId, CUSTOMER_TABLE, TICKET_TABLE,
};
use database::DATABASE;

pub async fn apply_open_service_confirm(apply: &OpenServiceConfirmApply) -> Result<(), String> {
    let customer_id = parse_record_id(&apply.customer_id, CUSTOMER_TABLE);
    let computer_id = canonical_computer_id(&apply.connection_string);
    let specs = &apply.resolved_specs;

    let customer_name = format!(
        "{} {}",
        apply.customer_first_name.trim(),
        apply.customer_last_name.trim()
    )
    .trim()
    .to_string();

    let customer = CustomerData {
        id: customer_id.clone(),
        name: if customer_name.is_empty() {
            apply.friendly_name.clone()
        } else {
            customer_name
        },
        ..CustomerData::default()
    };

    let _: Option<CustomerData> = DATABASE
        .upsert(customer.id.clone())
        .content(customer)
        .await
        .map_err(|e| format!("customer upsert: {e}"))?;

    let mut computer = computer_from_specs(specs, computer_id.clone(), customer_id.clone());
    if computer.hostname.is_empty() {
        if let Some((host, _)) = apply.connection_string.split_once(':') {
            computer.hostname = host.to_string();
        }
    }

    let _: Option<ComputerData> = DATABASE
        .upsert(computer.id.clone())
        .content(computer)
        .await
        .map_err(|e| format!("computer upsert: {e}"))?;

    let friendly = if apply.friendly_name.is_empty() {
        apply.candidate.service_number.clone()
    } else {
        apply.friendly_name.clone()
    };

    DATABASE
        .query(
            "UPDATE connected_client SET \
             customer = $cust, computer = $comp, friendly_name = $fname, \
             customer_locked = true, last_update = time::now() \
             WHERE connection_string == $cs RETURN AFTER",
        )
        .bind(("cust", customer_id.clone()))
        .bind(("comp", computer_id.clone()))
        .bind(("fname", friendly))
        .bind(("cs", apply.connection_string.clone()))
        .await
        .map_err(|e| format!("connected_client update: {e}"))?;

    // UPSERT with SET (not CONTENT) so we only touch the fields we own.
    // If the service_order row already exists with PrestaShop-sourced data
    // (sales_rep, checkin_rep, terms, ticket_total, etc.) those are left
    // intact; we only link the customer / computer records.
    let ticket_id = RecordId::new(TICKET_TABLE, apply.candidate.service_number.clone());
    DATABASE
        .query(
            "UPSERT $id SET \
             service_number = $sn, doc_alias = $alias, \
             checkin_notes = $notes, customer = $cust, computer = $comp",
        )
        .bind(("id", ticket_id))
        .bind(("sn", apply.candidate.service_number.clone()))
        .bind(("alias", apply.candidate.doc_alias.clone()))
        .bind(("notes", apply.candidate.checkin_notes.clone()))
        .bind(("cust", customer_id))
        .bind(("comp", computer_id))
        .await
        .map_err(|e| format!("service_order upsert: {e}"))?;

    Ok(())
}

fn computer_from_specs(
    specs: &PrestaSpecsSnapshot,
    id: RecordId,
    customer_id: RecordId,
) -> ComputerData {
    ComputerData {
        id,
        customer: Some(customer_id),
        cpu: specs.cpu.clone(),
        gpu: specs.gpu.clone(),
        ram: specs.ram.clone(),
        operating_system: specs.operating_system.clone(),
        device_serial: if specs.device_serial.is_empty() {
            None
        } else {
            Some(specs.device_serial.clone())
        },
        device_mfg: if specs.device_mfg.is_empty() {
            None
        } else {
            Some(specs.device_mfg.clone())
        },
        device_model: if specs.device_model.is_empty() {
            None
        } else {
            Some(specs.device_model.clone())
        },
        motherboard_name: specs.motherboard_name.clone(),
        ..ComputerData::default()
    }
}

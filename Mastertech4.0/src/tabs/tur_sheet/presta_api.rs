use database::schema::{
    EntityDraft, OrderLookup, PrestaMapOptions, fetch_prestashop_order, apply_prestashop_payload,
};
use crate::app_state::MastertechContext;

impl MastertechContext {
    pub fn presta_api(&self) {
        let input = self.ticket_data.service_number.clone();
        let phone = self.customer_data.phone_number.clone();
        let tx = self.prestashop_api_tx.clone();
        let lookup = if !input.is_empty() {
            Some(OrderLookup::ServiceNumber(input))
        } else if !phone.is_empty() {
            Some(OrderLookup::Phone(phone))
        } else {
            None
        };
        let Some(lookup) = lookup else { return };
        tokio::spawn(async move {
            let prestashop_order = fetch_prestashop_order(lookup).await?;
            tx.try_send(prestashop_order)?;
            Ok::<(), anyhow::Error>(())
        });
    }

    pub fn apply_prestashop_to_form(
        &mut self,
        data: &database::schema::prestashop_schema::PrestashopPayload,
        options: &PrestaMapOptions,
    ) {
        let mut draft = EntityDraft {
            customer: self.customer_data.clone(),
            ticket: self.ticket_data.clone(),
            computer: self.computer_data.clone(),
            task: self.task_data.clone(),
            task_notes: self.task_notes.clone(),
        };
        apply_prestashop_payload(data, &mut draft, options);
        self.customer_data = draft.customer;
        self.ticket_data = draft.ticket;
        self.computer_data = draft.computer;
        self.task_data = draft.task;
        self.task_notes = draft.task_notes;
    }
}

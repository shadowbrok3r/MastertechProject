use database::orders::tur_pull;
use database::schema::{
    EntityDraft, OrderLookup, PrestaMapOptions, apply_prestashop_payload,
};
use crate::app_state::MastertechContext;

impl MastertechContext {
    /// Pull the order named by the service-number field, or the customer's most
    /// recent order when only a phone number is filled in.
    ///
    /// Routes through both backends: a Shopify order number and a PrestaShop
    /// `id_order` both reach the sheet, and either outcome comes back on the
    /// channel so a failure is visible instead of silent.
    pub fn pull_order(&self) {
        let input = self.ticket_data.service_number.trim().to_string();
        let phone = self.customer_data.phone_number.trim().to_string();
        let tx = self.prestashop_api_tx.clone();
        let lookup = if !input.is_empty() {
            OrderLookup::ServiceNumber(input)
        } else if !phone.is_empty() {
            OrderLookup::Phone(phone)
        } else {
            let _ = tx.try_send(Err("Enter a service number or a phone number first".into()));
            return;
        };
        tokio::spawn(async move {
            let outcome = tur_pull::pull_order(lookup).await.map_err(|e| format!("{e:#}"));
            if let Err(e) = tx.try_send(outcome) {
                log::error!("order pull result could not be delivered: {e}");
            }
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

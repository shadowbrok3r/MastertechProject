use database::schema::utilities::{get_prestashop_payload, get_prestashop_payload_from_phone};
use crate::app_state::MastertechContext;

impl MastertechContext {
    pub fn presta_api(&self) {
        let input = self.ticket_data.service_number.clone();
        let phone = self.customer_data.phone_number.clone();
        let tx = self.prestashop_api_tx.clone();
        if !input.is_empty() {
            tokio::spawn(async move {
                let prestashop_order = get_prestashop_payload(&input).await?;
                tx.try_send(prestashop_order)?;
                Ok::<(), anyhow::Error>(())
            });
        } else if !phone.is_empty() {
            tokio::spawn(async move {
                let prestashop_order = get_prestashop_payload_from_phone(&phone).await?;
                tx.try_send(prestashop_order)?;
                Ok::<(), anyhow::Error>(())
            });  
        }
    }
}
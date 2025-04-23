use database::schema::utilities::create_full_task_payload;
use crate::app_state::MastertechContext;
use chrono::SecondsFormat;
use tokio::spawn;
use log::info;

impl MastertechContext {
    pub fn submit_tur_mastertech(&mut self) {
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        task_data.due_date = self.date.to_rfc3339_opts(SecondsFormat::Secs, true);
        let send_specs = self.send_specs.clone();

        spawn(async move {
            let send_payload_result = create_full_task_payload(
                ticket_data,
                customer_data,
                computer_data,
                task_data,
                task_notes,
                send_specs,
            )
            .await;
            info!("send_payload_result: {send_payload_result:?}");
        });
    }
}

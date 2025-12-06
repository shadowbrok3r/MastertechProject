use database::schema::{utilities::create_full_task_payload, TaskCreationResult};
use displays::{get_toast_sender, ToastMessage};
use crate::app_state::MastertechContext;
use tokio::spawn;
use log::info;

impl MastertechContext {
    pub fn submit_tur_mastertech(&mut self) {
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        task_data.due_date = self.date.into();
        let send_specs = self.send_specs.clone();
        let toast_tx = get_toast_sender();

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

            // Send toast based on result
            match send_payload_result {
                TaskCreationResult::Created { service_number } => {
                    let _ = toast_tx.try_send(ToastMessage::Success(
                        format!("Task created for service #{service_number}")
                    ));
                },
                TaskCreationResult::AlreadyExists { service_number } => {
                    let _ = toast_tx.try_send(ToastMessage::Warning(
                        format!("Task already exists for service #{service_number}")
                    ));
                },
                TaskCreationResult::Updated { service_number } => {
                    let _ = toast_tx.try_send(ToastMessage::Info(
                        format!("Task updated for service #{service_number}")
                    ));
                },
                TaskCreationResult::Error { message } => {
                    let _ = toast_tx.try_send(ToastMessage::Error(
                        format!("Error creating task: {message}")
                    ));
                },
            }
        });
    }
}

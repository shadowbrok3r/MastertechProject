use database::schema::{
    CarboniteResponse, HardwareTests, PrestaMapMode, PrestaMapOptions,
};
use crate::app_state::MasterTechApp;
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::Frame;

impl MasterTechApp {
    pub fn receive_prestashop(&mut self, frame: &mut Frame) {
        let Ok(outcome) = self.context.prestashop_api_rx.try_recv() else { return };

        let pulled = match outcome {
            Ok(pulled) => pulled,
            Err(message) => {
                log::warn!("order pull failed: {message}");
                self.context.shared_ctx.toasts.add(Toast {
                    kind: ToastKind::Error,
                    text: message.into(),
                    options: ToastOptions::default().show_progress(true).duration_in_seconds(10.0),
                    ..Default::default()
                });
                return;
            }
        };

        let origin = pulled.origin();
        let data = pulled.payload;
        self.context.service_details = data.order.associations.order_service.clone();
        let customer_email = data.customer.email.clone();
        let client = self.context.client.clone();
        let carobonite_tx = self.context.seb_channel.0.clone();
        let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
        let ram_test = format!("{:?}", &self.context.ram_test_cbox);
        let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);
        self.context.order_rows = data.order.associations.order_rows.clone();

        self.context.apply_prestashop_to_form(
            &data,
            &PrestaMapOptions {
                mode: PrestaMapMode::Bench,
                hardware_tests: Some(HardwareTests { hdd_test, ssd_test, ram_test }),
                ..Default::default()
            },
        );

        // Which backend answered decides what the sheet is missing, so it is
        // stated rather than left for the tech to infer from blank fields.
        self.context.shared_ctx.toasts.add(Toast {
            kind: ToastKind::Success,
            text: format!("Pulled order {} from {origin}", data.order.id).into(),
            options: ToastOptions::default().show_progress(true).duration_in_seconds(5.0),
            ..Default::default()
        });

        let customer_name = data.customer.name.clone();
        self.context
            .arm_assist_prompt(data.order.id.clone(), customer_name);

        tokio::spawn(async move {
            if !customer_email.is_empty() {
                log::warn!("Spawned thread, checking for CarboniteResponse");
                let response_json = CarboniteResponse::default()
                    .from_customer_email(customer_email.clone(), client)
                    .await;

                match response_json {
                    Ok(carbonite_response) => { let _ = carobonite_tx.try_send(carbonite_response); },
                    Err(e) => log::warn!("Error from carbonite response: {e:?}"),
                }
            }
        });

        log::warn!("receive_prestashop -> NOTES: {:#?}", self.context.task_notes);

        if let Some(storage) = frame.storage_mut() {
            storage.set_string(
                "ticket_data",
                serde_json::to_string(&self.context.ticket_data).unwrap_or_default(),
            );
            storage.set_string(
                "task_data",
                serde_json::to_string(&self.context.task_data).unwrap_or_default(),
            );
            storage.set_string(
                "customer_data",
                serde_json::to_string(&self.context.customer_data).unwrap_or_default(),
            );
        }
    }
}

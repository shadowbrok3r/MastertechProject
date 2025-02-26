use crate::{
    tabs::tur_sheet::get_ticket::SendRequest, 
    terminal_mode::events::action_handler::{ActionHandler, ApiEvent, WidgetEvent, WidgetId}
};

use database::schema::{
    utilities::PhoneNumberFormatter, GetKeysResponse
};

use super::ServiceFormWidget;

impl <'a> ActionHandler for ServiceFormWidget <'a>{
    fn widget_id(&self) -> WidgetId {
        WidgetId::ServiceForm
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id } if *widget_id == self.widget_id() => {
                if let Ok(svc_data) = &mut self.service_data.lock() {
                    log::info!("ServiceFormWidget handled a ButtonClick event.");
                    // Here you might access the input field's current value or trigger an API call.
                    let current_text = self.order_number.input.borrow().clone();
                    log::info!("Current order number: {}", current_text.lines()[0]);
                    svc_data.ticket_data.service_number = current_text.lines()[0].to_string();
                    svc_data.get_ticket();
                }
            }
            WidgetEvent::UpdateText { widget_id, text } if *widget_id == self.widget_id() => {
                log::info!("ServiceFormWidget received an UpdateText event: {}", text);
                let mut input = self.order_number.input.borrow_mut();
                input.select_all();
                input.cut();
                input.insert_str(text);
            }
            WidgetEvent::Api(ApiEvent::GetTicket(presta_data)) => {
                log::info!("GOT SOME MAIL");
                if let Ok(svc_data) = &mut self.service_data.lock() {

                    let _ = svc_data.receive(presta_data.clone());

                    let mut customer_name = self.customer_name.input.borrow_mut();
                    customer_name.select_all();
                    customer_name.cut();
                    customer_name.insert_str(svc_data.customer_data.name.clone());

                    let mut customer_phone = self.customer_phone.input.borrow_mut();
                    let mut formatter = PhoneNumberFormatter::default();
                    let phone_number = formatter
                        .format_phone_number(
                            &svc_data.customer_data.phone_number.clone()
                        )
                        .unwrap_or_default();
                    customer_phone.select_all();
                    customer_phone.cut();
                    customer_phone.insert_str(phone_number);

                    let mut salesman_name = self.salesman_name.input.borrow_mut();
                    salesman_name.select_all();
                    salesman_name.cut();
                    salesman_name.insert_str(svc_data.ticket_data.salesman.clone());

                    let mut technician_name = self.technician_name.input.borrow_mut();
                    technician_name.select_all();
                    technician_name.cut();
                    technician_name.insert_str(svc_data.ticket_data.tech.clone());

                    let mut checkin_notes = self.checkin_notes.input.borrow_mut();
                    checkin_notes.select_all();
                    checkin_notes.cut();
                    checkin_notes.insert_str(svc_data.ticket_data.checkin_notes.clone());

                    log::info!("SVC DATA: {svc_data:?}");
                }
            }
            WidgetEvent::Api(ApiEvent::SubmitTur) => {
                if let Ok(svc_data) = &mut self.service_data.lock() {
                    svc_data.submit_tur_mastertech();
                }
            }
            WidgetEvent::Api(ApiEvent::GetKeys) => {
                let (tx, rx) = crossbeam::channel::unbounded::<GetKeysResponse>();
                if let Ok(svc_data) = &mut self.service_data.lock() {
                    let cps_tx = tx.clone();
                    let service_num = self.order_number.input.borrow().clone();
                    svc_data.ticket_data.service_number = service_num.lines()[0].to_string();
                    let cps_request = SendRequest::get_cps(service_num.lines()[0].to_string(), self.client.clone());

                    tokio::spawn(async move{
                        let req =  cps_request.await.unwrap_or_default();
                        log::info!("Keys response: {req:?}");
                        let _ = cps_tx.send(req);
                    });

                    if let Ok(keys) = rx.recv() {
                        log::info!("Got keys: {keys:?}");
                        self.keys = keys.clone();
                        self.webroot_key_button.set_label(keys.webroot_key.clone());
                        self.superanti_key_button.set_label(keys.superanti_key.clone());
                    }
                }
            }
            WidgetEvent::Api(ApiEvent::CheckSeb) => {
                if let Ok(svc_data) = self.service_data.lock() {
                    let cust_email = &svc_data.customer_data.email;
                    if !cust_email.is_empty() {

                    }
                }
            }
            WidgetEvent::CopyWebroot => {
                let wrv = self.webroot_key_button.get_label();
                let mut clipboard = arboard::Clipboard::new().unwrap();
                let set = clipboard.set().text(wrv);
                log::info!("set text to clip: {set:?}");
            },
            WidgetEvent::CopySuperAnti => {
                let sas = self.superanti_key_button.get_label();
                let mut clipboard = arboard::Clipboard::new().unwrap();
                let set = clipboard.set().text(sas);
                log::info!("set text to clip: {set:?}");
            },
            _ => {}
        }
    }
}
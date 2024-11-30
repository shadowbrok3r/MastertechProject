use crate::app_state::MastertechContext;
use eframe::egui::Ui;

impl MastertechContext{
    pub fn special_part_order(&mut self, ui: &mut Ui) {
        let size_before_wrap = ui.available_size_before_wrap();
        let avail_size = [size_before_wrap.x - 30.0, size_before_wrap.y - 30.0];
        if let Some(usr) = &self.current_user {

            self.special_part_order.display_part_order_page(ui, avail_size.into(), usr.store);
            
            let name = &self.customer_data.name;
            let phone_number = &self.customer_data.phone_number;
            let service_number = &self.ticket_data.service_number;
            
            if name.len() > 0 && phone_number.len() > 0 && service_number.len() > 0 {
                self.special_part_order.set_customer(
                    self.customer_data.name.clone(),
                    self.customer_data.phone_number.clone(),
                    self.ticket_data.service_number.clone()
                );
            }
        }
    }
}
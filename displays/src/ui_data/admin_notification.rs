use eframe::egui::{Id, Modal};

use crate::app_state::SharedContext;


impl SharedContext {
    pub fn admin_notification_ui(&mut self, ctx: &eframe::egui::Context) {
        if let Some(notif) = &self.notification_modal {
            let modal = Modal::new(Id::new("Admin Notification")).show(ctx, |ui| {
                    ui.set_width(200.0);
                    ui.vertical_centered(|ui| {
                        ui.heading("Admin Notification");
                        ui.separator();
                        ui.add_space(10.);
                        ui.heading(notif.notification_description.clone());
                    });
                });

                if modal.should_close() {
                    self.notification_modal = None;
                }
            }
    }
}

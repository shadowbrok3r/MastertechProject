use eframe::egui::{Id, Label};

use crate::app_state::SharedContext;
use crate::ui_tools::{
    controls::{self, ModalWidth, Role},
    icons::svg,
};

impl SharedContext {
    pub fn admin_notification_ui(&mut self, ctx: &eframe::egui::Context) {
        let Some(notif) = &self.notification_modal else {
            return;
        };
        let out = controls::modal(
            ctx,
            Id::new("Admin Notification"),
            svg::MEGAPHONE,
            "Admin Notification",
            ModalWidth::Narrow,
            &[(Role::Primary, "OK")],
            |ui| {
                ui.add(Label::new(&notif.notification_description).wrap());
            },
        );
        if out.should_close || out.clicked.is_some() {
            self.notification_modal = None;
        }
    }
}

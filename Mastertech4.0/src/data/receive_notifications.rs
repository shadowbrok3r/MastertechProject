use crate::app_state::MasterTechApp;
// use database::live_data::{handle_live_delete, update_or_insert_anything};
// use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
// use eframe::egui::{Color32, FontId, RichText};
// use log::info;
// use surrealdb::Action;

impl MasterTechApp {
    pub fn receive_notification(&mut self) {
        // if let Ok((action, notification)) = self.context.live_notification_rx.try_recv() {
        //     info!("Action: {action:?} - Notification: {notification:?}");
        //     match action {
        //         Action::Create => {
        //             self.context.read_notifications = false;
        //             let toast = &mut self.context.toasts;
        //             let auth_toast = Toast {
        //                 kind: ToastKind::Info,
        //                 text: RichText::new(format!(
        //                     "Notification\n\n{}",
        //                     notification.notification_description
        //                 ))
        //                 .color(Color32::LIGHT_GREEN)
        //                 .font(FontId::proportional(15.))
        //                 .into(),
        //                 options: ToastOptions::default().duration(None),
        //             };
        //             toast.add(auth_toast);

        //             update_or_insert_anything(&mut self.context.notifications, notification.clone())
        //                 .unwrap_or(())
        //         }
        //         Action::Update => {
        //             update_or_insert_anything(&mut self.context.notifications, notification.clone())
        //                 .unwrap_or(())
        //         }
        //         Action::Delete => {
        //             handle_live_delete(&mut self.context.notifications, notification.clone())
        //                 .unwrap_or(())
        //         }
        //         _ => (),
        //     };
        // }

        // if let Ok(notification) = self.context.notification_rx.try_recv() {
        //     self.context.notifications = notification;
        // }
    }
}

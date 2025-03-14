use crate::terminal_mode::systems::notification_system::Notification;
use crossbeam::channel::{Receiver, Sender};
use std::{sync::{Arc, Mutex}, time::Duration};
use super::communication_system::{CommunicationSystem, Message};

/// Render system
pub struct RenderSystem {
    pub sender: Sender<Box<dyn Message>>,
    pub receiver: Receiver<Box<dyn Message>>,
    pub notifications: Arc<Mutex<Vec<Notification>>>,
    pub ui_messages: Arc<Mutex<Vec<Box<dyn Message>>>>,
}

impl RenderSystem {
    pub fn new(
        sender: Sender<Box<dyn Message>>,
        receiver: Receiver<Box<dyn Message>>,
    ) -> Self {
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let ui_messages = Arc::new(Mutex::new(vec![]));
        Self { sender, receiver, notifications, ui_messages }
    }

    pub async fn run(&self) {
        while let Ok(message) = self.receive() {
            log::info!("Message: {message:?}");
            if let Some(notification) = message.as_any().downcast_ref::<Notification>() {
                self.notifications.lock().unwrap().push(notification.clone());
                let notifications_clone = self.notifications.clone();
                // notification identification
                let notification_id = notification.id();
                let duration = notification.duration_secs;
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(duration)).await;
                    notifications_clone.lock().unwrap().retain(|n| n.id() != notification_id && !n.is_expired());
                });
            } else { // Store other UI messages for synchronous rendering like Vec<Box<dyn Message>>
                self.ui_messages.lock().unwrap().push(message);
            }
        }
    }
}


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

    pub async fn run(&self, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
        loop {
            // Check shutdown signal first (non-blocking)
            if let Ok(()) = shutdown_rx.recv().await {
                log::info!("Received Shutdown Signal");
                break;
            }
    
            // Run synchronous receive in a blocking context
            match tokio::task::block_in_place(|| self.receive()) {
                Ok(message) => {
                    log::info!("Message: {message:?}");
                    if let Some(notification) = message.as_any().downcast_ref::<Notification>() {
                        if let Ok(mut notif) = self.notifications.lock() {
                            notif.push(notification.clone());
                        }
                        let notifications_clone = self.notifications.clone();
                        let notification_id = notification.id();
                        let duration = notification.duration_secs;
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(duration)).await;
                            if let Ok(mut notif) = notifications_clone.lock() {
                                notif.retain(|n| n.id() != notification_id && !n.is_expired());
                            }
                        });
                    } else {
                        if let Ok(mut ui_msg) = self.ui_messages.lock() {
                            ui_msg.push(message);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("RenderSystem receive error: {:?}", e);
                    break; // Exit on error, adjust as needed
                }
            }
    
            // Small delay to yield control and prevent tight looping
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        log::info!("RenderSystem shutting down");
    }
}


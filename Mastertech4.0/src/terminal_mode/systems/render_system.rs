use crate::{filesystem::get_client_hash, terminal_mode::{context::TerminalContext, systems::{communication_system::DataMessage, notification_system::Notification}, websockets::create_client}};
use database::{WS_CLIENT_URL, schema::{utilities::get_tasks_for_store, RecordIdExt, User}};
use displays::get_database_users;
use std::{sync::{Arc, Mutex}, time::Duration};
use crossbeam::channel::{Receiver, Sender};
use super::communication_system::Message;

/// Render system
#[derive(Debug)]
pub struct RenderSystem {
    pub sender: Sender<Box<dyn Message>>,
    pub receiver: Receiver<Box<dyn Message>>,
    pub notifications: Arc<Mutex<Vec<Notification>>>,
    pub ui_messages: Arc<Mutex<Vec<Box<dyn Message>>>>,
    pub ctx: Arc<Mutex<TerminalContext>>,
}

impl RenderSystem {
    pub fn new(
        sender: Sender<Box<dyn Message>>,
        receiver: Receiver<Box<dyn Message>>,
        ctx: Arc<Mutex<TerminalContext>>,
    ) -> Self {
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let ui_messages = Arc::new(Mutex::new(vec![]));
        Self { sender, receiver, notifications, ui_messages, ctx }
    }

    pub async fn run(&self, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
        
        loop {
            tokio::select! {
                // Handle graceful shutdown (async)
                _ = shutdown_rx.recv() => {
                    log::info!("Received Shutdown Signal");
                    break;
                }
    
                // Properly handle synchronous blocking channel
                recv_result = tokio::task::spawn_blocking({
                    let receiver = self.receiver.clone();
                    move || receiver.recv()
                }) => {
                    match recv_result {
                        Ok(Ok(message)) => {
                            // log::info!("RenderSystem Received Message: {}", message.as_display());
    
                            if let Some(notification) = message.downcast_ref::<Notification>() {
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
                            } else if let Some(user) = message.downcast_ref::<DataMessage<User>>() {
                                if let Ok(mut ctx) = self.ctx.lock() {
                                    let tx = ctx.tasks_tx.clone();
                                    ctx.user = user.0.clone();
                                    let store = user.0.get_store().as_str().to_string();
                                    let usr_id = user.0.get_id().clone();
                                    let mut client = get_client_hash();
                                    let connection_url = format!(
                                        "{WS_CLIENT_URL}&room_id={}",
                                        client.id.key_string()
                                    );
                                    ctx.url = Some(connection_url.clone());
                                    ctx.store_users = get_database_users();
                                    let ctx_clone = self.ctx.clone();
                                    tokio::spawn(async move {

                                        client.assigned_user = Some(usr_id.clone());
                                        match create_client(client.clone()).await {
                                            Ok(created) => {
                                                log::info!("Client Creation OK, friendly_name: {:?}", created.friendly_name);
                                                if let Some(name) = &created.friendly_name {
                                                    if let Ok(mut ctx) = ctx_clone.lock() {
                                                        ctx.friendly_name = Some(name.clone());
                                                    }
                                                }
                                            }
                                            Err(e) => log::error!("Client Creation failed: {e:?}"),
                                        }
                                        
                                        let tasks_result = get_tasks_for_store(tx, store).await;
                                        log::info!("Tasks result: {tasks_result:?}");
                                    });
                                }
                            } else {
                                if let Ok(mut ui_msg) = self.ui_messages.lock() {
                                    ui_msg.push(message);
                                }
                            }
                        },
                        Ok(Err(e)) => {
                            log::warn!("Channel disconnected: {:?}", e);
                            break; // Break loop on receiver disconnect
                        },
                        Err(e) => {
                            log::warn!("spawn_blocking failed: {:?}", e);
                            break; // Break loop on unexpected error
                        }
                    }
                }
            }
        }
        log::info!("RenderSystem shutting down");
    }
    
}


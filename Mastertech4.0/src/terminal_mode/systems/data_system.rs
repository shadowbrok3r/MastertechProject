use crossbeam::channel::{Receiver, Sender};
use super::communication_system::{CommunicationSystem, Message};

// Data system
pub struct DataSystem {
    pub sender: Sender<Box<dyn Message>>,
    pub receiver: Receiver<Box<dyn Message>>,
}

impl DataSystem {
    pub fn new(
        sender: Sender<Box<dyn Message>>,
        receiver: Receiver<Box<dyn Message>>,
    ) -> Self {
        Self { sender, receiver }
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
                    log::info!("DataSystem received msg: {}", message.as_display());
                }
                Err(e) => {
                    log::warn!("DataSystem receive error: {:?}", e);
                    break; // Exit on error, adjust as needed
                }
            }
    
            // Small delay to prevent tight looping
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        log::info!("DataSystem shutting down");
    }
}
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

    pub async fn run(&self) {
        while let Ok(message) = self.receive() {
            log::info!("DataSystem received msg: {}", message.as_display());

        }
    }
}
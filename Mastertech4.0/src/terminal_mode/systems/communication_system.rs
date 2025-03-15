use std::{any::Any, fmt::Debug};

use database::schema::User;
use serde::Serialize;

use super::{data_system::DataSystem, notification_system::Notification, render_system::RenderSystem};

// Generic Message trait
pub trait Message: Send + Sync + Debug {
    fn as_display(&self) -> String;
    fn as_any(&self) -> &dyn Any;
}

// Communication trait for sending and receiving
pub trait CommunicationSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error>;
    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error>;
}


impl CommunicationSystem for DataSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error> {
        self.sender.send(msg)?;
        Ok(())
    }

    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error> {
        Ok(self.receiver.recv()?)
    }
}


impl CommunicationSystem for RenderSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error> {
        self.sender.send(msg)?;
        Ok(())
    }

    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error> {
        Ok(self.receiver.recv()?)
    }
}

impl Message for Notification {
    fn as_display(&self) -> String {
        format!("{}: {}", self.header, self.text)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}


// impl <T> DataMessage <T> 
//     where T: 
//         Serialize 
//         + Debug 
//         + Clone 
//         + Default
// {

// }
#[derive(Serialize, Debug, Clone, Default)]
pub struct DataMessage<T: Serialize + Debug + Clone + Default>(pub T);

impl Message for DataMessage<User> {
    fn as_display(&self) -> String {
        format!("{:?}", self.0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
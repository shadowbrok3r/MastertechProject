use std::{any::Any, fmt::Debug};

use database::schema::User;
use serde::Serialize;

use super::{data_system::DataSystem, notification_system::Notification, render_system::RenderSystem};

// Generic Message trait
pub trait Message: Send + Sync + Debug + Any {
    fn as_display(&self) -> String;
    // fn as_any(&self) -> &dyn Any;
}

impl dyn Message {
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        // Upcast is automatic here: &dyn MyAny to &dyn Any.
        (self as &dyn Any).downcast_ref()
    }
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
        let received = self.receiver.recv()?;
        log::info!("DataSystem got msg: {received:?}");
        Ok(received)
    }
}


impl CommunicationSystem for RenderSystem {
    fn send(&self, msg: Box<dyn Message>) -> anyhow::Result<(), anyhow::Error> {
        self.sender.send(msg)?;
        Ok(())
    }

    fn receive(&self) -> anyhow::Result<Box<dyn Message>, anyhow::Error> {
        let received = self.receiver.recv()?;
        log::info!("DataSystem got msg: {received:?}");
        Ok(received)
    }
}

impl Message for Notification {
    fn as_display(&self) -> String {
        format!("{}: {}", self.header, self.text)
    }

    // fn as_any(&self) -> &dyn Any {
    //     self
    // }
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

    // fn as_any(&self) -> &dyn Any {
    //     self
    // }
}
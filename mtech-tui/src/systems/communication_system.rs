use std::{any::Any, fmt::Debug};

use serde::Serialize;

use super::notification_system::Notification;

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
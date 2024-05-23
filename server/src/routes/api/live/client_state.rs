use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct Sessions(pub RwLock<HashMap<Uuid, Session>>);

/// Store Types
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub connected: bool
}

impl Session {
    pub fn new(username: String) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            username,
            connected: true
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Auth {
    pub session_id: Option<Uuid>,
    pub username: Option<String>,
    pub room: Store,
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy)]
pub enum Store{
    RIV,
    LTN,
    MUR,
    AF,
    WJ, 
    ORE,
    SAN
}

// pub type Clients = HashMap<String, VecDeque<ConnectedClient>>;


// #[derive(Default)]
// pub struct ClientStore {
//     pub clients: RwLock<Clients>,
// }

// impl ClientStore { // i should use a very similar setup for notifications
//     pub async fn insert(&self, room: &Store, client: ConnectedClient) {
//         debug!("inserting client_id {:?} into room {:?}", client.user_id.clone(), room.clone());
//         let mut binding = self.clients.write().await;
//         let clients = binding.entry(format!("{:?}", room.clone())).or_default();
//         clients.push_front(client);
//     }

//     pub async fn get(&self, room: &Store) -> Vec<ConnectedClient> {
//         let clients = self.clients.read().await.get(&format!("{:?}", room)).cloned();
//         clients.unwrap_or_default().into_iter().rev().collect()
//     }
//     // pub async fn remove(&self, room: &Store, client: ConnectedClient) {
//     //     debug!("remove client_id {:?} from room {:?}", client.user_id.clone(), room.clone());
//     //     let clients = self.clients.read().await.get(&format!("{:?}", room)).cloned();
        
//     //     clients.unwrap_or_default().remove(index);
//     // }
// }
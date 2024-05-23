use std::collections::{HashMap, VecDeque};
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/*
 *  TODO 
 *    - pending updates
 *    - cps activation
 */
#[derive (Clone, Serialize, Deserialize, Debug)]
pub struct ManagerMessage{
    /// Hostname of connected client machine + socket.id, if Role is Manager
    pub machine_id: String,
    /// Unique UUID for each connected client, will be stored in DB in near future
    // identifier: Option<Uuid>,
    /// Commands to send connected client machine
    pub command: Command,
    /// Role to determine whether connected peer MtechServer user, or Mastertech
    // pub role: Role,
    /// So people viewing the connected clients on the website ONLY see the  machines they or their store 
    /// are working on 
    pub room: String
}

#[derive (Clone, Serialize, Deserialize, Debug)]
pub struct ClientMessage{
    /// Hostname of connected client machine + socket.id, if Role is Manager
    pub machine_id: String,
    /// Role to determine whether connected peer MtechServer user, or Mastertech
    // pub role: String,
    /// So people viewing the connected clients on the website ONLY see the  machines they or their store 
    /// are working on 
    pub room: String,
    /// datetime of when the message was sent from the client
    pub date: Option<chrono::DateTime<chrono::Utc>>,

    pub sysinfo: SystemInformation,
}

#[derive (Clone, Serialize, Deserialize, Debug)]
pub struct CommandMessage{ 
    pub command: String,
    pub room: String,
}

/**
 * Command is the list of possible commands the Manager
 * Role is allowed to run on the connected client
 */
#[derive (Clone, Serialize, Deserialize, Debug)]
pub enum Command{
    /// Run a shell command (cmd, ps, bash on linux, etc)
    RunTask,
    ///  Used to retrieve live computer metrics such as temp, cpu usage, etc
    GetStats,
    /// List directory contents
    List    
}

/** 
 * Determine whether peer is a user of the 
 * mtechserver viewing connected clients, 
 * or if its mastertech sending the data to the server   
 */
#[derive (Clone, Serialize, Deserialize, Debug)]
pub enum Role{
    /// MtechServer user -> Manager Role
    Manager,
    /// Mastertech -> Client Role
    Client
}

#[derive (Clone, Serialize, Deserialize, Debug)]
pub struct SystemInformation{
    /// Live CPU usage as a percentage
    cpu_percentage: f32,
    /// Live CPU clock speed
    cpu_clock: u64,
    /// Live system temps
    component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb 
    used_memory: u64,
    /// Total RAM 
    total_memory: u64,
    /// Disk usage
    disks: String,
    /// Name of machine
    name: String,
    /// Kernel version
    kernel_version: String,
    /// OS version
    os_version: String,
    /// Hostname based on DNS
    pub hostname: String,
    /// Number of Physical CPU's
    number_of_cpus: String,

    network_interfaces: HashMap<String, String>
}
pub type RoomStore = HashMap<String, VecDeque<ClientMessage>>;

#[derive(Default)]
pub struct MessageStore {
    pub client_messages: RwLock<RoomStore>,
    // pub manager_messages: RwLock<RoomStore>
}

impl MessageStore {
    pub async fn _insert(&self, room: &String, message: ClientMessage) {
        let mut binding = self.client_messages.write().await;
        let messages = binding.entry(room.clone()).or_default();
        messages.push_front(message);
        messages.truncate(20);
    }

    pub async fn _get(&self, room: &String) -> Vec<ClientMessage> {
        let messages = self.client_messages.read().await.get(room).cloned();
        messages.unwrap_or_default().into_iter().rev().collect()
    }
}


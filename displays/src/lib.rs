use database::{schema::{ConnectedClient, Node, Priority, Status, Store, SystemInformation, TaskNotePayload, TaskPayload, TicketPayload, User}, CURRENT_USER_INFO, STORE_USERS};
use eframe::egui::{Modifiers, Response, Ui};
use bincode::{config::standard, serde::*};
use modals::task_modal::ModalAction;
use serde::{Deserialize, Serialize};
use crossbeam::channel::{Receiver, Sender};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use surrealdb::RecordId;
use egui_extras::Strip;
use std::fmt::Debug;
use once_cell::sync::Lazy;

pub mod virtual_filesystem;
pub mod channel_manager;
pub mod egui_data_table;
pub mod markdown_editor;
pub mod file_viewer;
pub mod viewports;
pub mod app_state;
pub mod first_run;
pub mod ui_tools;
pub mod ui_data;
pub mod modals;
pub mod views;
pub mod tasks;
pub mod chats;
pub mod tabs;
pub mod ai;

#[cfg(feature = "tokio")]
pub mod remote_viewer;

pub use platform::PlatformSpawner;

#[cfg(target_arch="wasm32")]
pub use {
    // rayon_wasm::prelude::{self as rayon},
    async_openai_wasm::{self as openai}
};
#[cfg(not(target_arch="wasm32"))]
pub use {
    // rayon::prelude::{self as rayon},
    async_openai::{self as openai}
};

// Define a global event sender (wrapped in `Arc<Mutex<T>>` for safe access)
static GLOBAL_USERS_CHANNEL: Lazy<(Sender<Vec<User>>, Receiver<Vec<User>>)> = Lazy::new(|| crossbeam::channel::unbounded());

pub fn get_users_channel_sender() -> Sender<Vec<User>> {
    GLOBAL_USERS_CHANNEL.0.clone()
}

pub fn get_users_channel_receiver() -> Receiver<Vec<User>> {
    GLOBAL_USERS_CHANNEL.1.clone()
}


pub trait Spawner {
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static;

    #[cfg(target_arch = "wasm32")]
    fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + 'static;
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::Spawner;
    use wasm_bindgen_futures::spawn_local;

    pub struct PlatformSpawner;

    impl Spawner for PlatformSpawner {
        fn spawn<F>(future: F)
        where
            F: std::future::Future<Output = ()> + 'static,
        {
            spawn_local(future);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::Spawner;
    use tokio::task;

    pub struct PlatformSpawner;

    impl Spawner for PlatformSpawner {
        fn spawn<F>(future: F)
        where
            F: std::future::Future<Output = ()> 
                + 'static 
                + std::marker::Send,
                
        {
            task::spawn(future);
        }
    }
}


pub fn get_current_user_from_auth() -> Option<User> {
    if let Ok(current_user) = CURRENT_USER_INFO.try_lock() {
        // log::warn!("WE HAVE A USER FROM GLOBAL STATE");
        current_user.clone()
    } else {
        log::warn!("NONE");
        None
    }
}

pub fn get_database_users()  -> Vec<User>{
    if let Ok(users) = STORE_USERS.try_lock() {
        // log::warn!("WE HAVE STORE USERS FROM GLOBAL STATE");
        users.clone()
    } else {
        log::warn!("NONE");
        vec![]
    }
}


#[derive(Debug, Clone)]
pub enum TaskUiActions {
    OpenTaskModal(TaskPayload),
    CreateTaskModal,
    OpenChatModal((RecordId, Vec<TaskNotePayload>, Option<String>)),
    OpenViewport(TaskPayload),
    None,
}


pub trait Displayable {
    fn display_cards(&mut self, user: &User, ui: &mut Ui, store_users: &Vec<User>, tx: Sender<TaskUiActions>);
}


pub trait ColumnLayout {
    fn layout_cols(&mut self, ui: &mut Ui);
    fn columns(&mut self, s: &mut Strip);
    fn headers(&mut self, s: Strip);
    // fn card_layout(&mut self, uir &mut Ui) -> Option<TaskUiActions>;
}

#[async_trait]
pub trait Updatable {
    async fn update_service_number(&self, service_number: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_completed(&self, completed: bool) -> anyhow::Result<(), anyhow::Error>;
    async fn update_due_date(&self) -> anyhow::Result<(), anyhow::Error>;
    async fn update_assignee_initials(&self, initials: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error>;
    async fn update_dep(&self, store: Store) -> anyhow::Result<(), anyhow::Error>;
    async fn update_priority(&self, priority: Option<Priority>) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_description(&self) -> anyhow::Result<(), anyhow::Error>;
    async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Interaction {
    fn interact_service_number(&mut self, ui: &mut Ui) -> Response;
    fn interact_task_name(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_task_description(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_checkin_notes(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_due_date(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_completed(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_status(&mut self, user: &User, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_priority(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_assignee_initials(&mut self, ui: &mut Ui, store_users: &Vec<User>) -> Response; // , task: Rc<RefCell<TaskPayload>>
}

pub trait FilterTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<TaskPayload>;
    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<TaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<TaskPayload>;
}
pub trait FilterClients {
    fn filter_by_client<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<ConnectedClient>;
}

pub trait Sortable <T> {
    fn default_sort(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<T>;
}

pub trait LiveUpdate {
    fn handle_live_create(
        self,
        existing_tasks: &mut Vec<TaskPayload>,
        new_ticket: Option<TicketPayload>,
    ) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_update(
        self,
        existing_tasks: &mut Vec<TaskPayload>,
        new_ticket: Option<TicketPayload>,
    ) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_delete(
        self,
        existing_tasks: &mut Vec<TaskPayload>,
        new_ticket: Option<TicketPayload>,
    ) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
}

#[async_trait]
pub trait Task {
    // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>);
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    // fn create_data(&mut self, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn get_data(&mut self, data: T)    -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn modify_data(&mut self, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn delete_data(&mut self, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
}

pub trait DisplayModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction>;
    // fn set_state(self, action: ModalAction);
}

#[derive(Default, PartialEq, Clone, serde::Serialize)]
pub enum SortDirection{
    #[default]
    Asc,
    Desc
}


#[derive(Serialize, Deserialize, Debug)]
pub enum Cmd {
    LiveData,
    Command,
    Tuneup,
    Cps,
    Qc,
    SfcScan,
    DismScan,
    ChkDsk,
    Mbr2Gpt,
    TaskManager,
    FileSystemAction(FileSystemAction),
    UninstallProgram(String),
    PullKeys(String),
    PullTicket(String),
    InteractiveInput(String),
    CheckSeb,
    QuitInteractive,
    ReadEvents,
    Quit,
    None,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FileSystemAction{
    Execute(String),
    CopyToClient(String),
    CopyFromClient(String),
    Delete(String),
    Select((Modifiers, String)),
    PreviewedFile(String),
    EnterDirectory(String),
    ExpandDirectory(String),
    GetNode(Node),
    RequestNewContents(String),
    NavigateHome,
}

pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    encode_to_vec(system_info, standard()).expect("Failed to serialize SystemInformation")
}

pub fn deserialize_command(bytes: &[u8]) -> Cmd {
    let (cmd, _) = decode_from_slice(bytes, standard()).expect("Failed to deserialize Cmd");
    cmd
}
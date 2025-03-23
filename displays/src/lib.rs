use database::schema::{ConnectedClient, Node, Priority, Status, Store, SystemInformation, TaskNotePayload, TaskPayload, TicketPayload, User};
use eframe::egui::{Modifiers, Response, Ui};
use modals::task_modal::ModalAction;
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use async_trait::async_trait;
use surrealdb::RecordId;
use egui_extras::Strip;
use std::fmt::Debug;

pub mod file_viewer;
pub mod channel_manager;
pub mod egui_data_table;
pub mod markdown_editor;
pub mod modals;
pub mod ui_tools;
// #[cfg(not(target_arch = "wasm32"))]
pub mod remote_viewer;
pub mod views;
pub mod virtual_filesystem;
pub mod tasks;
pub mod chats;
pub mod tabs;
pub mod app_state;
pub mod ui_data;
pub mod first_run;
pub mod ai;
pub mod viewports;
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


#[derive(Debug, Clone)]
pub enum TaskUiActions {
    OpenTaskModal(TaskPayload),
    CreateTaskModal,
    OpenChatModal((RecordId, Vec<TaskNotePayload>)),
    Response(Response),
    Editing(RecordId),
    CommitChanges(RecordId),
    OpenViewport(TaskPayload),
    None,
}

/* 
// In your crate with `egui`
pub trait EguiRenderable {
    fn render(&self, ui: &mut egui::Ui) -> egui::Response;
}

// In your `database::schema` crate
use crate::egui::EguiRenderable;

impl EguiRenderable for Notification {
    fn render(&self, ui: &mut egui::Ui) -> egui::Response {
        ui.label(self.notification_description)
    }
}
impl Widget for &mut Notification {
    fn ui(self, ui: &mut eframe::egui::Ui) -> eframe::egui::Response {
        ui.label(self.notification_description)
    }
} 
*/


pub trait Displayable {
    fn display_cards(&mut self, ui: &mut Ui, store_users: &Vec<User>, tx: Sender<TaskUiActions>);
}


pub trait ColumnLayout {
    fn layout_cols(&mut self, ui: &mut Ui);
    fn columns(&mut self, s: &mut Strip);
    fn headers(&mut self, s: Strip);
    // fn card_layout(&mut self, uir &mut Ui) -> Option<TaskUiActions>;
}

#[async_trait]
pub trait Updatable {
    // This is correctly implemented
    async fn update_completed(&self, completed: bool) -> anyhow::Result<(), anyhow::Error>;
    async fn update_due_date(&self, due_date: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_assignee_initials(&self, initials: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error>;
    async fn update_dep(&self, store: Store) -> anyhow::Result<(), anyhow::Error>;
    async fn update_priority(&self, priority: Option<Priority>) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_description(&self, description: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_checkin_notes(&self, checkin_notes: Option<String>) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_notes(&self, new_msg: String) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Interaction {
    // This is correctly implemented
    fn interact_task_name(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_task_description(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_checkin_notes(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_due_date(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_completed(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_status(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_priority(&mut self, ui: &mut Ui) -> Response; // , task: Rc<RefCell<TaskPayload>>
    fn interact_assignee_initials(&mut self, ui: &mut Ui, store_users: &Vec<User>) -> Response; // , task: Rc<RefCell<TaskPayload>>
}

pub trait FilterTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: &String) -> Vec<TaskPayload>;
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
    bincode::serialize(system_info).expect("Failed to serialize SystemInformation")
}

pub fn _deserialize_system_info(bytes: &[u8]) -> SystemInformation {
    bincode::deserialize(bytes).expect("Failed to deserialize SystemInformation")
}

pub fn deserialize_command(bytes: &[u8]) -> Cmd {
    bincode::deserialize(bytes).expect("Failed to deserialize Cmd")
}
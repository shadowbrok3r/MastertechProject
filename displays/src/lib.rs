use database::{schema::{LiveTaskPayload, Node, Priority, Status, SystemInformation, TaskNotePayload, User}, CURRENT_USER_INFO, STORE_USERS};
use eframe::egui::{Modifiers, Response, Ui};
use crossbeam::channel::{Receiver, Sender};
use bincode::{config::standard, serde::*};
use modals::task_modal::ModalAction;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use surrealdb::RecordId;
use egui_extras::Strip;
use std::fmt::Debug;

pub mod virtual_filesystem;
pub mod channel_manager;
pub mod markdown_editor;
pub mod file_viewer;
pub mod viewports;
pub mod app_state;
pub mod ui_tools;
pub mod ui_data;
pub mod scripts;
pub mod modals;
pub mod pages;
pub mod chats;
pub mod tabs;
pub mod ai;

pub use platform::PlatformSpawner;

#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;

#[cfg(feature = "tokio")]
pub mod remote_viewer;


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


pub const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Proportional"},"Body":{"size":14.0,"family":"Proportional"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":true,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;

/// Toast message type for displaying notifications from async contexts
#[derive(Debug, Clone)]
pub enum ToastMessage {
    Success(String),
    Error(String),
    Warning(String),
    Info(String),
}

// Define a global event sender (wrapped in `Arc<Mutex<T>>` for safe access)
static GLOBAL_USERS_CHANNEL: Lazy<(Sender<Vec<User>>, Receiver<Vec<User>>)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Global channel for toast messages from async contexts
static GLOBAL_TOAST_CHANNEL: Lazy<(Sender<ToastMessage>, Receiver<ToastMessage>)> = Lazy::new(|| crossbeam::channel::unbounded());

pub fn get_users_channel_sender() -> Sender<Vec<User>> {
    GLOBAL_USERS_CHANNEL.0.clone()
}

pub fn get_users_channel_receiver() -> Receiver<Vec<User>> {
    GLOBAL_USERS_CHANNEL.1.clone()
}

/// Get the sender for toast messages (use from async contexts)
pub fn get_toast_sender() -> Sender<ToastMessage> {
    GLOBAL_TOAST_CHANNEL.0.clone()
}

/// Get the receiver for toast messages (use in UI update loop)
pub fn get_toast_receiver() -> Receiver<ToastMessage> {
    GLOBAL_TOAST_CHANNEL.1.clone()
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
        log::trace!("get_current_user_from_auth: user retrieved from global state");
        current_user.clone()
    } else {
        log::warn!("get_current_user_from_auth: failed to acquire lock");
        None
    }
}

pub fn get_database_users() -> Vec<User> {
    if let Ok(users) = STORE_USERS.try_lock() {
        log::trace!("get_database_users: users retrieved from global state");
        users.clone()
    } else {
        log::warn!("get_database_users: failed to acquire lock");
        vec![]
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskUiActions {
    OpenTaskModal(LiveTaskPayload),
    CreateTaskModal,
    OpenChatModal((RecordId, Vec<TaskNotePayload>, Option<String>)),
    OpenViewport(LiveTaskPayload),
    None,
}


pub trait Displayable {
    fn display_cards(
        &mut self, 
        ui: &mut Ui, 
        user: &User, 
        store_users: &Vec<User>, 
        notes: Vec<TaskNotePayload>,
        tx: Sender<TaskUiActions>
    );
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
    async fn update_assignee(&self, assignee: RecordId) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error>;
    async fn update_priority(&self, priority: Option<Priority>) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_description(&self) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Interaction {
    fn interact_service_number(&mut self, ui: &mut Ui) -> Response;
    fn interact_task_name(&mut self, ui: &mut Ui) -> Response; 
    fn interact_task_description(&mut self, ui: &mut Ui) -> Response; 
    fn interact_due_date(&mut self, ui: &mut Ui) -> Response; 
    fn interact_completed(&mut self, ui: &mut Ui) -> Response; 
    fn interact_status(&mut self, user: &User, ui: &mut Ui) -> Response; 
    fn interact_priority(&mut self, ui: &mut Ui) -> Response; 
    fn interact_assignee(&mut self, ui: &mut Ui, store_users: &Vec<User>, current_user: &User) -> Response; 
}


#[async_trait]
pub trait Task {
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
}

pub trait DisplayModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction>;
}


#[derive(Serialize, Deserialize, Debug)]
pub enum Cmd {
    LiveData,
    TaskManager,
    FileSystemAction(FileSystemAction),
    UninstallProgram(String),
    PullKeys(String),
    PullTicket(String),
    InteractiveInput(String),
    /// Execute a shell command with specified shell type
    ShellCommand { command: String, shell: ShellCommandType },
    /// Start an interactive shell session with specified shell type
    StartInteractiveShell(ShellCommandType),
    QuitInteractive,
    ReadEvents,
    Quit,
    None,
}

/// Shell command type for cross-platform shell execution
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCommandType {
    /// Windows PowerShell
    PowerShell,
    /// Windows CMD
    Cmd,
    /// Unix Bash
    Bash,
    /// Unix sh
    Sh,
    /// Auto-detect based on OS
    Auto,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FileSystemAction {
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
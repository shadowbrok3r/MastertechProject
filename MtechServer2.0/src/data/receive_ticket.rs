use crate::{
    app_state::{AppState, MtechServer},
    pages::downloads_page::get_github_releases,
    tabs::stock::{find_attached_serials, get_stock, BoolOrString, MyRowData},
    utilities::ModalType,
};
use database::{
    live_data::{handle_live_delete, listen_data, update_or_insert_anything},
    schema::{
        utilities::{get_connected_clients, get_store_users, get_tasks},
        TaskNotePayload, CONNECTED_CLIENT_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE,
    },
    DATABASE,
};
use database::{schema::Store, STORAGE_URL};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::{
    egui::{Color32, RichText},
    Frame,
};
use egui_dock::DockState;
use log::info;
use log::{debug, error};
use mtechserver::webworker::Input;
use surrealdb::{Action, RecordId};
use wasm_bindgen_futures::spawn_local;

// #[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // mtechserver::live_worker::LiveInput,
};
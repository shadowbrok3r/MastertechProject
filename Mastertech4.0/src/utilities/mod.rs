pub mod app_restart;
pub mod safe_swap;
pub mod crypto;
pub mod ui_action;
pub mod scripts;
pub mod ai;
pub mod network;

pub use crypto::{load_encrypted_user_data, save_encrypted_user_data};

#[cfg(target_os="windows")]
pub mod windows;
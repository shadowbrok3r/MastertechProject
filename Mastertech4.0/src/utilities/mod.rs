pub mod app_restart;
pub mod safe_swap;
pub mod crypto;
pub mod ui_action;
pub mod scripts;
// `ai` (the :9001 DesktopToolProvider) removed: an unauthenticated loopback endpoint
// inside an elevated process is a local privilege-escalation primitive. Its
// replacement is `remote_exec`, which rides the authenticated admin session.

pub mod network;

pub use crypto::{load_encrypted_user_data, save_encrypted_user_data};

#[cfg(target_os="windows")]
pub mod windows;
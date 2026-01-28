//! Modal dialogs for terminal mode
//! 
//! This module contains popup modals that can be displayed over the main UI.

mod duplicate_merge_modal;
pub mod task_modal;

pub use duplicate_merge_modal::*;
pub use task_modal::TaskModal;

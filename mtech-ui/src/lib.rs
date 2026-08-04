//! Shared egui UI helpers for Mastertech apps: semantic theme colors, egui_dock
//! chrome, and the in-app logger. Used by `displays` (MasterTech) and `qc_app`.

pub mod dock_style;
pub mod egui_logger;
pub mod github;
pub mod icons;
#[cfg(feature = "stress-ui")]
pub mod stress_dashboard;
pub mod theme;

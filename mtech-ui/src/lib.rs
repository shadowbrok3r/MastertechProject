//! Shared egui UI helpers for Mastertech apps: semantic theme colors, egui_dock
//! chrome, and the in-app logger. Used by `displays` (MasterTech) and `qc_app`.

pub mod controls;
pub mod dock_style;
pub mod egui_logger;
pub mod fonts;
pub mod github;
pub mod icon_pack;
pub mod icons;
pub mod metrics;
#[cfg(feature = "stress-ui")]
pub mod stress_dashboard;
pub mod theme;
pub mod title_bar;
pub mod tokens;
pub mod widget_theme;

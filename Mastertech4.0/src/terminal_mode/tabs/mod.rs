pub mod service_order;
pub mod sysinfo;

////////////////////////////////////
// Main App Code
////////////////////////////////////

#[derive(Debug, Clone, Copy)]
pub enum Tab {
    TurSheet,
    Scripts,
    SystemInfo,
    Extra,
}
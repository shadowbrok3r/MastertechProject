pub mod antivirus;
pub mod installed_programs;
pub mod net_adapter;
#[cfg(target_os = "windows")]
pub mod reboot;
pub mod registry;
pub mod windows_update;
pub mod drivers;
// pub mod nvme;
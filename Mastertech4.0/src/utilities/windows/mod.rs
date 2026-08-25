pub mod antivirus;
#[cfg(target_os = "windows")]
pub mod crash_dumps;
pub mod installed_programs;
pub mod net_adapter;
pub mod power;
#[cfg(target_os = "windows")]
pub mod reboot;
pub mod registry;
pub mod windows_update;
pub mod drivers;
// pub mod nvme;
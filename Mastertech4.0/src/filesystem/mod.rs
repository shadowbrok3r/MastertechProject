pub mod system_info;
pub mod machine;

// Wrap the Nvml instance in lazy_static
lazy_static::lazy_static! {
    static ref NVML_INSTANCE: tokio::sync::Mutex<nvml_wrapper::Nvml> = tokio::sync::Mutex::new(
        nvml_wrapper::Nvml::init().expect("Failed to initialize NVML")
    );
    static ref SYSINFO: tokio::sync::Mutex<sysinfo::System> = tokio::sync::Mutex::new(
        sysinfo::System::new_all()
    );
}
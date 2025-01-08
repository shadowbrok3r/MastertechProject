pub mod system_info;
pub mod machine;

// Wrap the Nvml instance in lazy_static
lazy_static::lazy_static! {
    static ref NVML_INSTANCE: std::sync::Arc<nvml_wrapper::Nvml> = std::sync::Arc::new(
        nvml_wrapper::Nvml::init().expect("Failed to initialize NVML")
    );
    static ref SYSINFO: std::sync::Arc<tokio::sync::Mutex<sysinfo::System>> = std::sync::Arc::new(
        tokio::sync::Mutex::new(sysinfo::System::new_all())
    );
}

static MACHINE_INSTANCE: std::sync::OnceLock<std::sync::Arc<machine::Machine>> = std::sync::OnceLock::new();

pub async fn get_machine_instance() -> &'static std::sync::Arc<machine::Machine> {
    let machine = machine::Machine::new(
        std::sync::Arc::clone(&NVML_INSTANCE),
        std::sync::Arc::clone(&SYSINFO),
    ).await;
    
    MACHINE_INSTANCE.get_or_init(|| {
        std::sync::Arc::new(machine)
    })
}
use database::schema::{ConnectedClient, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE};
use surrealdb::RecordId;
use sysinfo::System;
use system_info::generate_client_id;

pub mod system_info;
pub mod machine;

// Wrap the Nvml instance in lazy_static
lazy_static::lazy_static! {
    static ref SYSINFO: std::sync::Arc<tokio::sync::Mutex<System>> = std::sync::Arc::new(
        tokio::sync::Mutex::new(System::new_all())
    );
}

pub fn get_client_hash() -> ConnectedClient {
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let cpu = sys.cpus()[0].brand().trim().to_string();
    let hostname = System::host_name().unwrap_or_default();

    let client_hash = generate_client_id(hostname.clone(), cpu.trim().to_string());
    let id = format!("{}:{}", hostname.clone(), client_hash.split_at(9).0);

    let client_id = RecordId::from_table_key(
        CONNECTED_CLIENT_TABLE.to_string(), 
        id.clone().as_str()
    );

    let computer_id = RecordId::from_table_key(
        COMPUTER_TABLE.to_string(), 
        id.clone().as_str()
    );

    ConnectedClient {
        id: client_id.clone(),
        client_hash,
        connected: false,
        connection_string: id.clone(),
        computer: Some(computer_id.clone()),
        ..Default::default()
    }
}

static MACHINE_INSTANCE: std::sync::OnceLock<std::sync::Arc<machine::Machine>> = std::sync::OnceLock::new();

pub async fn get_machine_instance() -> Result<&'static std::sync::Arc<machine::Machine>, nvml_wrapper::error::NvmlError> {
    log::info!("Initializing NVML");
    // Initialize NVML_INSTANCE inside the function to handle potential errors
    let nvml_instance = nvml_wrapper::Nvml::init().map(std::sync::Arc::new)?;

    log::info!("Initializing Machine Instance");
    // Use SYSINFO and NVML_INSTANCE to create the machine instance
    let machine = machine::Machine::new(
        nvml_instance,
        std::sync::Arc::clone(&SYSINFO),
    ).await;

    // Get or initialize the machine instance
    Ok(MACHINE_INSTANCE.get_or_init(|| {
        std::sync::Arc::new(machine)
    }))
}
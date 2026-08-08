use database::schema::{ConnectedClient, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE};
use system_info::generate_client_id;
use database::schema::RecordId;
use sysinfo::System;

pub mod system_info;
#[cfg(target_os = "windows")]
pub mod oa_serial;
#[cfg(target_os = "windows")]
pub mod customer_lookup;
pub mod machine;

// Wrap the Nvml instance in lazy_static
lazy_static::lazy_static! {
    static ref SYSINFO: std::sync::Arc<tokio::sync::Mutex<System>> = std::sync::Arc::new(
        tokio::sync::Mutex::new(System::new_all())
    );
}

/// Process-wide cache for the deterministic `ConnectedClient`
/// identity (id / connection_string / client_hash / computer link).
///
/// Hostname + CPU brand don't change while the process is running, so
/// hashing them once at first call and reusing the result is correct.
/// The pre-cache shape ran `System::new_all()` + `sys.refresh_all()`
/// on **every** invocation, which:
///
///   1. floods the log with `generate_client_id` lines (each call
///      emits two log lines from inside `generate_client_id`), and
///   2. blocks the tokio runtime worker for the duration of the
///      sysinfo scan — which on Windows includes process / disk /
///      memory enumeration and can take 100s of ms.  With many
///      callers (reachability prober ⇒ `handle_session` ⇒
///      `get_client_hash`; terminal-mode menu_bar/render ⇒ per-frame;
///      spawn_direct_tcp_listener ⇒ per-bind) the runtime worker
///      pool got starved enough that the TCP listener bind task
///      sometimes never finished `spawn_blocking` and the listener
///      never came up.
///
/// The cache fixes both: first call still does the scan, subsequent
/// calls clone the cached value in nanoseconds.
static CLIENT_HASH_CACHE: std::sync::OnceLock<ConnectedClient> = std::sync::OnceLock::new();

pub fn get_client_hash() -> ConnectedClient {
    CLIENT_HASH_CACHE
        .get_or_init(|| {
            // First call only — the expensive scan.  Still uses
            // `System::new_all()` because we want the freshest possible
            // CPU brand string at process-start time (it can include
            // tier/SKU detail that isn't present in cached PROCESSOR_
            // env vars).  Subsequent calls skip all this.
            let mut sys = System::new_all();
            sys.refresh_all();

            let cpu = sys
                .cpus()
                .first()
                .map(|c| c.brand().trim().to_string())
                .unwrap_or_default();
            // Under WinPE this is the offline install's hostname, not `HBCD_PE`,
            // so the key matches that machine's normal Windows check-in.
            let hostname = stress_runner::identity_hostname();
            let boot_environment = stress_runner::boot_environment();

            let client_hash = generate_client_id(hostname.clone(), cpu.trim().to_string());
            let id = format!("{}:{}", hostname.clone(), client_hash.split_at(9).0);

            let client_id = RecordId::new(
                CONNECTED_CLIENT_TABLE.to_string(),
                id.clone(),
            );
            let computer_id = RecordId::new(
                COMPUTER_TABLE.to_string(),
                id.clone(),
            );

            log::info!(
                "client identity: {id} (boot_environment={})",
                boot_environment.as_str()
            );

            // Lets the notification pump recognise this machine's own rows.
            displays::set_local_connection_string(id.clone());

            ConnectedClient {
                id: client_id,
                client_hash,
                connected: false,
                connection_string: id,
                computer: Some(computer_id),
                boot_environment,
                ..Default::default()
            }
        })
        .clone()
}

/// Cached `computer` record id — same key as `ConnectedClient.computer`.
pub fn local_computer_record() -> RecordId {
    get_client_hash()
        .computer
        .expect("get_client_hash always sets computer")
}

static MACHINE_INSTANCE: std::sync::OnceLock<std::sync::Arc<machine::Machine>> = std::sync::OnceLock::new();

pub async fn get_machine_instance() -> Result<&'static std::sync::Arc<machine::Machine>, nvml_wrapper::error::NvmlError> {
    log::debug!("Initializing NVML");
    // Initialize NVML_INSTANCE inside the function to handle potential errors
    let nvml_instance = nvml_wrapper::Nvml::init().map(std::sync::Arc::new)?;

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
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default )]
pub struct SystemInformation {
    pub cpu: String,
    pub motherboard_name: String,
    pub motherboard_serial: String,
    pub motherboard_asset_tag: String,
    pub motherboard_vendor: String,
    pub product_name: String,
    pub product_sku: String,
    pub product_serial: String,
    pub product_vendor: String,
    /// Live CPU usage as a percentaget
    pub cpu_percentage: f32,
    /// Live CPU clock speed
    pub cpu_clock: f32,
    /// Live system temps
    pub component_temps: std::collections::HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: f32,
    /// Total RAM
    pub total_memory: f32,
    /// Disk usage
    pub disks: Vec<Disk>,
    /// Name of machine
    pub name: String,
    /// Kernel version
    pub kernel_version: String,
    /// OS version
    pub os_version: String,
    /// Hostname based on DNS
    pub hostname: String,
    /// Number of Physical CPU's
    pub number_of_cpus: String,
    /// list of network interfaces and 
    pub network_interfaces: Vec<NetworkInterface>,
    /// List of active processes on host
    pub processes: Vec<Process>,
    pub gpu_info: Gpu,
    /// Windows Hardware Error Architecture counters polled from the
    /// system event log. `None` on non-Windows or when the WHEA
    /// channel isn't readable (e.g. running unprivileged). The
    /// stress-kit telemetry agent maintains these in the background
    /// and the LiveData loop snapshots them into every payload.
    ///
    /// **Do NOT add `skip_serializing_if` here.** This struct is
    /// serialized via bincode `standard()`, which is a positional
    /// format — `skip_serializing_if` makes the encoder omit the
    /// field when `None`, and the decoder then over-reads past the
    /// buffer end and silently fails (every sysinfo payload gets
    /// dropped, live charts go blank). `Option<T>` already encodes
    /// as a 1-byte tag + payload regardless of variant, so leaving
    /// the field unconditional costs at most one byte per payload.
    /// `#[serde(default)]` stays so JSON / debug formats deserialize
    /// cleanly when the field is missing in those contexts.
    #[serde(default)]
    pub whea: Option<WheaCounters>,
    /// GPU TDR (Timeout Detection and Recovery) counters from Windows.
    /// Same source as `whea` and same `None` semantics. Same
    /// no-`skip_serializing_if` rule as `whea` — see comment above.
    #[serde(default)]
    pub tdr: Option<TdrCounters>,
    /// Per-logical-core live samples for remote charting. Empty on
    /// older clients until they upgrade the LiveData builder.
    #[serde(default)]
    pub cpu_cores: Vec<CpuCoreLive>,
}

/// Per-logical-core row on the LiveData wire (mirrors stress-kit `CoreSample`).
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct CpuCoreLive {
    pub index: usize,
    pub usage_pct: f32,
    pub freq_mhz: u64,
    #[serde(default)]
    pub temp_c: Option<f32>,
}

/// Mirror of `stress_kit::telemetry::WheaCounters` carried over the
/// wire so the admin's live view doesn't have to depend on stress-kit
/// types. `delta_since_program_start` increments while the client
/// process has been running; `absolute_since_boot` is the OS-wide
/// running total since the machine booted.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct WheaCounters {
    pub delta_since_program_start: u64,
    pub absolute_since_boot: u64,
}

/// Mirror of `stress_kit::telemetry::TdrCounters`. Same shape as
/// `WheaCounters`; kept as a separate type so a future change to one
/// doesn't accidentally affect the other.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct TdrCounters {
    pub delta_since_program_start: u64,
    pub absolute_since_boot: u64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default )]
pub struct Disk {
    pub device_name: String,
    pub file_system: String,
    pub mount_point: String,
    pub total_space: u64,
    pub available_space: u64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Gpu {
    pub usage: Vec<GraphicsUsage>,
    pub card: Vec<GraphicsCard>
}

/// Graphic card usage by process
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsProcessUtilization {
    /// Process identificator
    pub pid: u32,
    /// Gpu identificator
    pub gpu: u32,
    /// Memory usage
    pub memory: u32,
    /// Gpu encoder utilization as percentage
    pub encoder: u32,
    /// Gpu decoder utilization as percentage
    pub decoder: u32    
}

/// Graphic card usage summary
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsUsage {
    /// Graphic card id
    pub id: String,
    /// Memory utilization as percentage
    pub memory_usage: u32,
    /// Memroy usage as bytes
    pub memory_used: u64,
    /// Gpu encoder utilization as percentage
    pub encoder: u32,
    /// Gpu decoder utilization as percentage
    pub decoder: u32,
    /// Gpu utilization as percentage
    pub gpu: u32,
    /// Gpu temperature
    pub temperature: u32,
    /// Processes using this GPU
    pub processes: Vec<GraphicsProcessUtilization>
}

/// Information about a graphic card
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsCard {
    /// Device id
    pub id: String,
    /// Device id
    pub name: String,
    /// Device brand
    pub brand: String,
    /// Total memory
    pub memory: u64,
    /// Device temperature
    pub temperature: u32,
    pub nvidia_info: NvidiaInfo
}


/// Nvidia drivers configuration
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NvidiaInfo {
     /// Nvidia drivers
     pub driver_version: String,
     /// NVML version
     pub nvml_version: String,
     /// Cuda version
     pub cuda_version: i32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Default)]
pub struct Process {
    /// Process ID
    pub id: u32,
    pub name: String,
    pub cmd: String,
    pub user_id: Option<String>,
    pub memory: f32,
    pub cpu_usage: f32,
    pub process_disk_usage: ProcessDiskUsage,
    /// Path to the executable (if available)
    pub exe_path: Option<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Default)]
pub struct ProcessDiskUsage {
    pub read_bytes: f32,
    pub total_read_bytes: f32,
    pub total_written_bytes: f32,
    pub written_bytes: f32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Default)]
pub struct NetworkInterface {
    /// Process ID
    pub interface_name: String,
    pub total_received: f32,
    pub total_transmitted: f32,
}

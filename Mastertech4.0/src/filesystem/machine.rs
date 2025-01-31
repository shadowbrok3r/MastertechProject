use database::schema::{GraphicsCard, GraphicsProcessUtilization, GraphicsUsage, NvidiaInfo};
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, Nvml};
use sysinfo::{Disks, System};
use tokio::sync::Mutex;
// use tokio::sync::Mutex;
use std::sync::Arc;


/// Represents a machine. Currently you can monitor global CPU/Memory usage, processes CPU usage and the
/// Nvidia GPU usage. You can also retrieve information about CPU, disks...
pub struct Machine {
    nvml: Arc<Nvml>,
    pub sysinfo: Arc<Mutex<System>>,
    // Cache for static fields
    _cache: Arc<std::sync::Mutex<CachedSystemInformation>>,
}


impl Machine {
    /// Creates a new instance of Machine. If not graphic card it will warn about it but not an error
    /// Example
    /// ```
    /// use machine_info::Machine;
    /// let m = Machine::new();
    /// ```
    pub async fn new(nvml: Arc<Nvml>, sysinfo: Arc<Mutex<System>>) -> Self {
        let sysclone = sysinfo.clone();
        let sys = sysclone.lock().await;
        let _cache = Arc::new(std::sync::Mutex::new(
            CachedSystemInformation::new(&sys)
        ));
        
        Machine {
            nvml,
            sysinfo,
            _cache,
        }
    }
    
    /// Retrieves basic GraphicsCard information
    /// Example
    /// ```
    /// let m = machine_info::Machine::new();
    /// log::info!("{:?}", m.gpu_info())
    /// ```
    pub fn gpu_info(&self) -> anyhow::Result<Vec<GraphicsCard>, anyhow::Error> {
        let mut cards = Vec::new();
        let nvml = &self.nvml;
        for n in 0..nvml.device_count()? {
            let device = nvml.device_by_index(n)? ;
            cards.push(GraphicsCard{
                id: device.uuid()?,
                name: device.name()?,
                brand: match device.brand()? {
                    nvml_wrapper::enum_wrappers::device::Brand::GeForce => "GeForce".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::Quadro => "Quadro".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::Tesla => "Tesla".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::Titan => "Titan".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::NVS => "NVS".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::GRID => "GRID".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::NvidiaRTX => "NvidiaRTX".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::QuadroRTX => "QuadroRTX".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::Nvidia => "Nvidia".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::GeForceRTX => "GeForceRTX".to_string(),
                    nvml_wrapper::enum_wrappers::device::Brand::TitanRTX => "TitanRTX".to_string(),
                    _ => "Unknown/VPC/VPW".to_string()
                },
                memory: device.memory_info()?.total,
                temperature: device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)?,
                nvidia_info: NvidiaInfo {
                    driver_version: nvml.sys_driver_version().unwrap(),
                    nvml_version: nvml.sys_nvml_version().unwrap(),
                    cuda_version: nvml.sys_cuda_driver_version().unwrap()
                }
            });
        }
        Ok(cards)
    }

    /// The current usage of all graphic cards (if any)
    /// Example
    /// ```
    /// let m = machine_info::Machine::new();
    /// log::info!("{:?}", m.graphics_status())
    /// ```
    pub fn graphics_status(&self) -> anyhow::Result<Vec<GraphicsUsage>, anyhow::Error> {
        let mut cards = Vec::new();
        let nvml = &self.nvml;
        for n in 0..nvml.device_count()? {
            let device = nvml.device_by_index(n)?;
            let mut processes = Vec::new();
            let stats = device.process_utilization_stats(None);
            if stats.is_ok() {
                for p in stats? {
                    processes.push(GraphicsProcessUtilization{
                        pid: p.pid,
                        gpu: p.sm_util,
                        memory: p.mem_util,
                        encoder: p.enc_util,
                        decoder: p.dec_util
                    });
                }
            }

            cards.push(GraphicsUsage {
                id: device.uuid()?,
                memory_used: device.memory_info()?.used,
                encoder: device.encoder_utilization()?.utilization,
                decoder: device.decoder_utilization()?.utilization,
                gpu: device.utilization_rates()?.gpu,
                memory_usage: device.utilization_rates()?.memory,
                temperature: device.temperature(TemperatureSensor::Gpu)?,
                processes
            });
        }
        Ok(cards)
    }
}

pub struct CachedSystemInformation {
    pub _name: String,
    pub _os_version: String,
    pub _kernel_version: String,
    pub _hostname: String,
    pub _disks: String,
    pub _total_memory: f32,
}

impl CachedSystemInformation {
    pub fn new(sys: &sysinfo::System) -> Self {
        // Populate only once with values that don't change
        let name = System::name().unwrap_or_default();
        let os_version = System::os_version().unwrap_or_default();
        let kernel_version = System::kernel_version().unwrap_or_default();
        let hostname = System::host_name().unwrap_or_default();
        
        let mut disks = String::new();
        for disk in Disks::new_with_refreshed_list().iter() {
            disks += format!("{disk:?}\n").as_str();
        }
        
        let total_memory = sys.total_memory() as f32 / (1024.0 * 1024.0);
        
        CachedSystemInformation {
            _name: name,
            _os_version: os_version,
            _kernel_version: kernel_version,
            _hostname: hostname,
            _disks: disks,
            _total_memory: total_memory,
        }
    }
}

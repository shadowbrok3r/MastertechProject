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
    cache: Arc<std::sync::Mutex<CachedSystemInformation>>,
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
        let cache = Arc::new(std::sync::Mutex::new(
            CachedSystemInformation::new(&sys)
        ));
        
        Machine {
            nvml,
            sysinfo,
            cache,
        }
    }
    
    /// Retrieves full information about the computer
    /// Example
    /// ```
    /// use machine_info::Machine;
    /// let m = Machine::new();
    /// println!("{:?}", m.system_info())
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
    /// use machine_info::Machine;
    /// let m = Machine::new();
    /// println!("{:?}", m.graphics_status())
    /// ```
    pub fn graphics_status(&self) -> Vec<GraphicsUsage> {
        let mut cards = Vec::new();
        let nvml = &self.nvml;
        for n in 0..nvml.device_count().unwrap() {
            let device = nvml.device_by_index(n).unwrap();
            let mut processes = Vec::new();
            let stats = device.process_utilization_stats(None);
            if stats.is_ok() {
                for p in stats.unwrap() {
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
                id: device.uuid().unwrap(),
                memory_used: device.memory_info().unwrap().used,
                encoder: device.encoder_utilization().unwrap().utilization,
                decoder: device.decoder_utilization().unwrap().utilization,
                gpu: device.utilization_rates().unwrap().gpu,
                memory_usage: device.utilization_rates().unwrap().memory,
                temperature: device.temperature(TemperatureSensor::Gpu).unwrap(),
                processes
            });
        }
        cards
    }
}

pub struct CachedSystemInformation {
    pub name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub hostname: String,
    pub disks: String,
    pub total_memory: f32,
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
            name,
            os_version,
            kernel_version,
            hostname,
            disks,
            total_memory,
        }
    }
}

// #[async_trait]
// pub trait SysInf {
//     fn init_machine(&mut self);
//     fn get_cpu(&mut self);
//     fn get_gpu(&mut self);
//     fn get_memory(&mut self);
//     fn get_disks(&mut self);
//     fn get_processes(&mut self);
//     fn get_components(&mut self);
//     fn get_static_info(&mut self);
//     fn get_network_interfaces(&mut self);
// }

// pub async fn get_sysinfo() -> anyhow::Result<SystemInformation, anyhow::Error> {
//     // let mut sys = System::new_all();

//     // // First we update all information of our `System` struct.
//     // sys.refresh_all();

//     let mut machine = Machine::new().await;
//     let card = machine.gpu_info()?;
//     let usage = machine.graphics_status();

//     let gpu_info = Gpu {
//         card,
//         usage
//     };
    
//     let mut sys = machine.sysinfo;
//     info!("GPU: {gpu_info:?}");
//     sys.refresh_all();
//     let mut cpu_percentage = f32::default();
//     let mut cpu_clock = f32::default();
//     let mut disks = String::new();
//     let disk_list = Disks::new_with_refreshed_list();
//     let mut network_interfaces: Vec<NetworkInterface> = Vec::new();
//     let mut component_temps: HashMap<String, f32> = HashMap::new();
//     let mut processes: Vec<SysProcess> = Vec::new();
//     // Components temperature:
//     let components = Components::new_with_refreshed_list();
//     // Network interfaces name, total data received and total data transmitted:
//     let networks = Networks::new_with_refreshed_list();
//     // RAM and swap information:
//     let total_memory = sys.total_memory() as f32 / (1024.0 * 1024.0);
//     let used_memory = sys.used_memory() as f32 / (1024.0 * 1024.0);

//     // Display system information:
//     let name = System::name().context("Could not retrieve system name")?;
//     let kernel_version = System::kernel_version().context("Could not retrieve kernel_version")?;
//     let os_version = System::os_version().context("Could not retrieve os_version")?;
//     let hostname = System::host_name().context("Could not retrieve hostname")?;

//     // Number of CPUs:
//     let number_of_cpus = format!("NB CPUs: {} \n", sys.cpus().len());
    
//     // Display processes ID, name na disk usage:
//     // for (pid, process) in sys.processes() {println!("[{pid}] {:?} {:?}", process.name(), process.disk_usage());}
//     for (pid, process) in sys.processes().iter() {
//         let id = pid.as_u32();
//         let name = process.name().to_string_lossy().to_string();
//         let cmd = format!("{:?}", process.cmd());
//         let user_id = process.user_id().map(|id| id.to_string());
        
//         let memory = (process.memory() as f32 / (1024.0 * 1024.0) * 100.0).round() / 100.0;

//         let cpu_usage = process.cpu_usage();
//         let read_bytes = (process.disk_usage().read_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
//         let total_read_bytes = (process.disk_usage().total_read_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
//         let total_written_bytes = (process.disk_usage().total_written_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
//         let written_bytes = (process.disk_usage().written_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;

//         processes.push(SysProcess {
//             id,
//             name,
//             cmd,
//             user_id,
//             memory,
//             cpu_usage,
//             process_disk_usage: ProcessDiskUsage {
//                 read_bytes,
//                 total_read_bytes,
//                 total_written_bytes,
//                 written_bytes,
//             },
//         });
//     }

//     for disk in &disk_list {
//         disks += format!("{disk:?}").as_str();
//     }

//     for (interface_name, data) in &networks {
//         if data.total_received() > 1 {
//             let interface_name = interface_name.to_string();
//             network_interfaces.push(
//                 NetworkInterface { 
//                     interface_name,
//                     total_received: (data.total_received() as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0,
//                     total_transmitted: (data.total_transmitted() as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0
//                 }
//             );
//         }
//     }

//     for component in &components {
//         component_temps.insert(component.label().to_string(), component.temperature().unwrap_or_default());
//         // comps += format!("{component:#?} \n", component.).as_str();
//     }

//     let mut s = System::new_with_specifics(RefreshKind::everything());

//     tokio::time::sleep(Duration::from_millis(200)).await;

//     s.refresh_cpu_all(); // Refreshing CPU information.
//     for cpu in s.cpus() {
//         cpu_percentage = cpu.cpu_usage();
//         cpu_clock = cpu.frequency() as f32;
//     }

//     Ok(SystemInformation {
//         cpu_percentage,
//         cpu_clock,
//         component_temps,
//         disks,
//         total_memory,
//         used_memory,
//         name,
//         kernel_version,
//         os_version,
//         hostname,
//         number_of_cpus,
//         network_interfaces,
//         processes,
//         gpu_info,
//     })
// }
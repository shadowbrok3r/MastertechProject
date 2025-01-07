use database::schema::{GraphicsCard, GraphicsProcessUtilization, GraphicsUsage, NvidiaInfo};
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, Nvml};
use super::{NVML_INSTANCE, SYSINFO};
use tokio::sync::MutexGuard;
use sysinfo::System;


/// Represents a machine. Currently you can monitor global CPU/Memory usage, processes CPU usage and the
/// Nvidia GPU usage. You can also retrieve information about CPU, disks...
pub struct Machine<'a> {
    nvml: Option<MutexGuard<'a, Nvml>>,
    pub sysinfo: MutexGuard<'a, System>,
}


impl<'a> Machine<'a> {
    /// Creates a new instance of Machine. If not graphic card it will warn about it but not an error
    /// Example
    /// ```
    /// use machine_info::Machine;
    /// let m = Machine::new();
    /// ```
    pub async fn new() -> Machine<'a> {
        let nvml = NVML_INSTANCE.lock().await;
        let sysinfo = SYSINFO.lock().await;
        
        Machine {
            nvml: Some(nvml),
            sysinfo,
        }
    }
    
    /// Retrieves full information about the computer
    /// Example
    /// ```
    /// use machine_info::Machine;
    /// let m = Machine::new();
    /// println!("{:?}", m.system_info())
    /// ```
    pub fn gpu_info(& mut self) -> anyhow::Result<Vec<GraphicsCard>, anyhow::Error> {
        let mut cards = Vec::new();
        if let Some(nvml) = &self.nvml {
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
        } else {
            Ok(Vec::new())
        }
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
        if let Some(nvml) = &self.nvml {
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
        }
        cards
    }
}
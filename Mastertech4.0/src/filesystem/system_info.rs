use database::schema::{ComputerData, DriveData, Gpu, LocalSebData, NetworkInterface, Process as SysProcess, ProcessDiskUsage, SystemInformation, COMPUTER_TABLE};
use crate::{filesystem::get_machine_instance, tabs::tur_sheet::get_ticket::request_seb_info};
use std::{collections::HashMap, env, str, sync::Arc, time::Duration};
use sysinfo::{Components, Disks, Networks, System};
use num_format::{Locale, ToFormattedString};
use tokio::{io::{self, ErrorKind}, spawn};
use crossbeam::channel::Sender;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use surrealdb::RecordId;
use log::{error, info};
use reqwest::Client;
use anyhow::Context;
use super::SYSINFO;

#[cfg(target_os = "windows")]
use crate::{terminal_mode::tabs::script_checks::check_windows_activation, utilities::scripts::InstalledProgram};

pub const CREATE_NO_WINDOW: u32 = 0x08000000;

#[async_trait]
pub trait ComputerInfo {
    async fn get_computer_data(&mut self) -> anyhow::Result<ComputerData, anyhow::Error>;
    #[allow(unused)]
    async fn get_computer_data_no_gpu(&mut self) -> anyhow::Result<ComputerData, anyhow::Error>;
    #[allow(unused)]
    async fn get_sysinfo() -> anyhow::Result<SystemInformation, anyhow::Error>;
    #[cfg(target_os = "windows")]
    fn get_antivirus() -> io::Result<Vec<(String, Option<bool>)>>;
}

#[async_trait]
impl ComputerInfo for ComputerData {
    async fn get_computer_data(&mut self) -> anyhow::Result<Self, anyhow::Error> {
        info!("Filesystem -> get_computer_data -> Getting sysinfo");
        
        let sys = &mut SYSINFO.lock().await;
        // info!("GPU: {gpu_info:?}");
        sys.refresh_all();
        
        info!("Filesystem -> get_computer_data -> Pulling Drive information");
        let mut disks = Disks::new_with_refreshed_list();
        let client = Client::new();

        for disk in &mut disks {
            if !disk.is_removable() {
                self.add_disk(DriveData {
                    drive_type: format!("{:?}", disk.kind()),
                    total_size: (disk.total_space() / (1024 * 1024 * 1024))
                        .to_formatted_string(&Locale::en),
                    space_left: (disk.available_space() / (1024 * 1024 * 1024))
                        .to_formatted_string(&Locale::en),
                    drive_letter: disk.mount_point().to_str().unwrap_or("").to_string(),
                });
                info!("Filesystem -> get_computer_data -> DriveData: {:?}", disk.name());
            }
        }

        let seb_data: Result<LocalSebData, anyhow::Error> = request_seb_info(client, None)
            .await
            .or_else(|err| {
                error!("Error Pulling SEB info: {:?}", err.to_string());
                Err(err)
            })
            .and_then(|data| {
                info!("Filesystem -> get_computer_data -> Pulled SEB Data successfully: {data:#?}");
                Ok(data)
            });

        #[cfg(target_os = "windows")]
        {
            info!("Filesystem -> get_computer_data -> pulling GPU");
            // Using Powershell instead using Get-CimInstance because wmic is deprecated in favor
            // of it
            let process = tokio::process::Command::new("powershell")
                .args(["-C", "(Get-CimInstance Win32_VideoController).Name.Trim()"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await;

            info!("Filesystem -> get_computer_data -> Process: {process:?}");

            if let Ok(out) = process {
                info!("Filesystem -> get_computer_data -> out: {out:?}");
                let gpu = String::from_utf8(out.stdout).unwrap_or(String::new());
                info!("Filesystem -> get_computer_data -> GPU: {gpu:?}");
                self.gpu = gpu.clone().trim().to_string();
            }

        }

        #[cfg(target_os = "linux")]
        {
            info!("Filesystem -> get_computer_data -> Pulling linux gpu");
            let re = regex::Regex::new(r"\[(.*)\]").unwrap();
            let gpu = String::from_utf8(
                tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("lspci | grep VGA")
                    .output()
                    .await?
                    .stdout,
            );

            if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str())
            {
                let full_gpu_name = &captures[1];
                self.gpu = full_gpu_name
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<&str>>()
                    .join(" ");
            }
        }

        if let Ok(seb_info) = seb_data {
            self.seb_info = Some(seb_info);
        }

        info!("Filesystem -> get_computer_data -> Pulling CPU");
        self.cpu = sys.cpus()[0].brand().trim().to_string();
        info!("Filesystem -> get_computer_data -> Pulling RAM");
        self.ram = (sys.total_memory() / (1024 * 1024 * 1024) + 1)
            .to_formatted_string(&Locale::en)
            .trim()
            .to_string();
        info!("Filesystem -> get_computer_data -> Pulling OS");
        self.operating_system = System::long_os_version().unwrap_or_default();
        info!("Filesystem -> get_computer_data -> Pulling Hostname");
        self.hostname = System::host_name().unwrap_or_default();

        let client_hash = generate_client_id(self.hostname.clone(), self.cpu.trim().to_string());
        let id = format!("{}:{}", self.hostname.clone(), client_hash.split_at(9).0);
        info!("Filesystem -> get_computer_data -> ID: {id}");
        self.id = RecordId::from((COMPUTER_TABLE, id.clone().as_str()));
        info!("Filesystem -> get_computer_data -> RecordID: {:?}", self.id.clone());

        #[cfg(target_os="windows")]
        {
            // let installed_programs = InstalledProgram::get_installed_programs()?;
            // if let Ok(programs) = serde_json::to_value(installed_programs) {
            //     self.installed_programs = Some(programs);
            // }

            let license_status = check_windows_activation()?;
            if license_status.license_status == 1 {
                self.windows_active = Some(true);
            } else {
                self.windows_active = Some(false);
            }
        }

        Ok(self.to_owned())
    }

    async fn get_computer_data_no_gpu(&mut self) -> anyhow::Result<Self, anyhow::Error> {
        info!("Filesystem -> get_computer_data -> Getting sysinfo");
    
        let _gpu_info = Gpu::default();
        
        let sys = &mut SYSINFO.lock().await;
        // info!("GPU: {gpu_info:?}");
        sys.refresh_all();
        
        info!("Filesystem -> get_computer_data -> Pulling Drive information");
        let mut disks = Disks::new_with_refreshed_list();
        let client = Client::new();

        for disk in &mut disks {
            if !disk.is_removable() {
                self.add_disk(DriveData {
                    drive_type: format!("{:?}", disk.kind()),
                    total_size: (disk.total_space() / (1024 * 1024 * 1024))
                        .to_formatted_string(&Locale::en),
                    space_left: (disk.available_space() / (1024 * 1024 * 1024))
                        .to_formatted_string(&Locale::en),
                    drive_letter: disk.mount_point().to_str().unwrap_or("").to_string(),
                });
                info!("Filesystem -> get_computer_data -> DriveData: {:?}", disk.name());
            }
        }

        let seb_data: Result<LocalSebData, anyhow::Error> = request_seb_info(client, None)
            .await
            .or_else(|err| {
                error!("Error Pulling SEB info: {:?}", err.to_string());
                Err(err)
            })
            .and_then(|data| {
                info!("Filesystem -> get_computer_data -> Pulled SEB Data successfully: {data:#?}");
                Ok(data)
            });

        #[cfg(target_os = "windows")]
        {
            info!("Filesystem -> get_computer_data -> pulling GPU");
            // Using Powershell instead using Get-CimInstance because wmic is deprecated in favor
            // of it
            let process = tokio::process::Command::new("powershell")
                .args(["-C", "(Get-CimInstance Win32_VideoController).Name.Trim()"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await;

            info!("Filesystem -> get_computer_data -> Process: {process:?}");

            let x = process.unwrap().stdout;
            info!("Filesystem -> get_computer_data -> x: {x:?}");

            let gpu = String::from_utf8(x).unwrap_or(String::new());
            info!("Filesystem -> get_computer_data -> GPU: {gpu:?}");
            self.gpu = gpu.clone().trim().to_string();
        }

        #[cfg(target_os = "linux")]
        {
            info!("Filesystem -> get_computer_data -> Pulling linux gpu");
            let re = regex::Regex::new(r"\[(.*)\]").unwrap();
            let gpu = String::from_utf8(
                tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("lspci | grep VGA")
                    .output()
                    .await?
                    .stdout,
            );

            if let Some(captures) = re.captures(gpu.clone().unwrap_or("empty".to_string()).as_str())
            {
                let full_gpu_name = &captures[1];
                self.gpu = full_gpu_name
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<&str>>()
                    .join(" ");
            }
        }

        if let Ok(seb_info) = seb_data {
            self.seb_info = Some(seb_info);
        }

        info!("Filesystem -> get_computer_data -> Pulling CPU");
        self.cpu = sys.cpus()[0].brand().trim().to_string();
        info!("Filesystem -> get_computer_data -> Pulling RAM");
        self.ram = (sys.total_memory() / (1024 * 1024 * 1024) + 1)
            .to_formatted_string(&Locale::en)
            .trim()
            .to_string();
        info!("Filesystem -> get_computer_data -> Pulling OS");
        self.operating_system = System::long_os_version().unwrap_or_default();
        info!("Filesystem -> get_computer_data -> Pulling Hostname");
        self.hostname = System::host_name().unwrap_or_default();

        let client_hash = generate_client_id(self.hostname.clone(), self.cpu.trim().to_string());
        let id = format!("{}:{}", self.hostname.clone(), client_hash.split_at(9).0);
        info!("Filesystem -> get_computer_data -> ID: {id}");
        self.id = RecordId::from((COMPUTER_TABLE, id.clone().as_str()));
        info!("Filesystem -> get_computer_data -> RecordID: {:?}", self.id.clone());
        Ok(self.to_owned())
    }

    async fn get_sysinfo() -> anyhow::Result<SystemInformation, anyhow::Error> {
        Ok(get_sysinfo().await?)
    }

    #[cfg(target_os = "windows")]
    fn get_antivirus() -> io::Result<Vec<(String, Option<bool>)>> {
        let (sender, receiver) = crossbeam::channel::unbounded();

        let av_to_search = vec![
            "mbam",             // MALWAREBYTES
            "aswtoolssvc",      // AVAST
            "avgToolsSvc",      // AVG
            "mcuicnt",          // MCAFFEE
            "norton",           // NORTON
            "wrsa",             // WEBROOT
            "egui",             // ESET
            "superantispyware", // SUPERANTI
        ];

        let antivirus_mapping = Arc::new(
            [
                ("mbam", "Malwarebytes"),
                ("aswtoolssvc", "Avast"),
                ("avgToolsSvc", "AVG"),
                ("mcuicnt", "McAfee"),
                ("norton", "Norton"),
                ("wrsa", "Webroot"),
                ("egui", "ESET"),
                ("superantispyware", "SuperAntiSpyware"),
            ]
            .iter()
            .cloned()
            .collect::<HashMap<&str, &str>>(),
        );

        for antivirus in av_to_search.clone().into_iter() {
            let sender = sender.clone();
            let antivirus_mapping = Arc::clone(&antivirus_mapping);

            spawn(async move {
                let where_cmd = ["where", "/r", "C:\\Program Files", antivirus];

                let output = tokio::process::Command::new("cmd")
                    .args(&["/C"])
                    .args(where_cmd)
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .await
                    .map_err(|e| {
                        io::Error::new(
                            ErrorKind::Other,
                            format!("Failed to execute command: {}", e),
                        )
                    })?;

                let exists = if output.stdout.is_empty() {
                    None
                } else {
                    Some(true)
                };

                let name = antivirus_mapping.get(antivirus).unwrap_or(&antivirus);

                sender.try_send((name.to_string(), exists)).map_err(|_| {
                    io::Error::new(ErrorKind::BrokenPipe, "Failed to send data through channel")
                })
            });
        }

        let mut antivirus_exists = Vec::new();
        for _ in 0..av_to_search.len() {
            let exists = receiver.try_recv().map_err(|_| {
                io::Error::new(ErrorKind::BrokenPipe, "Failed to receive data from channel")
            })?;
            antivirus_exists.push(exists);
        }

        Ok(antivirus_exists)
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

pub async fn get_sysinfo() -> anyhow::Result<SystemInformation, anyhow::Error> {
    let machine = get_machine_instance().await?.clone();
    let card = machine.gpu_info()?;
    let usage = machine.graphics_status()?;

    let gpu_info = Gpu {
        card,
        usage
    };
    
    let sys = &mut machine.sysinfo.lock().await;
    // info!("GPU: {gpu_info:?}");
    sys.refresh_all();
    let mut cpu_percentage = f32::default();
    let mut cpu_clock = f32::default();
    let mut disks = String::new();
    let disk_list = Disks::new_with_refreshed_list();
    let mut network_interfaces: Vec<NetworkInterface> = Vec::new();
    let mut component_temps: HashMap<String, f32> = HashMap::new();
    let mut processes: Vec<SysProcess> = Vec::new();
    // Components temperature:
    let components = Components::new_with_refreshed_list();
    // Network interfaces name, total data received and total data transmitted:
    let networks = Networks::new_with_refreshed_list();
    // RAM and swap information:
    let total_memory = sys.total_memory() as f32 / (1024.0 * 1024.0);
    let used_memory = sys.used_memory() as f32 / (1024.0 * 1024.0);

    // Display system information:
    let name = System::name().context("Could not retrieve system name")?;
    let kernel_version = System::kernel_version().context("Could not retrieve kernel_version")?;
    let os_version = System::os_version().context("Could not retrieve os_version")?;
    let hostname = System::host_name().context("Could not retrieve hostname")?;

    // Number of CPUs:
    let number_of_cpus = format!("NB CPUs: {} \n", sys.cpus().len());
    
    // Display processes ID, name na disk usage:
    // for (pid, process) in sys.processes() {log::info!("[{pid}] {:?} {:?}", process.name(), process.disk_usage());}
    for (pid, process) in sys.processes().iter() {
        let id = pid.as_u32();
        let name = process.name().to_string_lossy().to_string();
        let cmd = format!("{:?}", process.cmd());
        let user_id = process.user_id().map(|id| id.to_string());
        
        let memory = (process.memory() as f32 / (1024.0 * 1024.0) * 100.0).round() / 100.0;

        let cpu_usage = process.cpu_usage();
        let read_bytes = (process.disk_usage().read_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
        let total_read_bytes = (process.disk_usage().total_read_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
        let total_written_bytes = (process.disk_usage().total_written_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
        let written_bytes = (process.disk_usage().written_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;

        processes.push(SysProcess {
            id,
            name,
            cmd,
            user_id,
            memory,
            cpu_usage,
            process_disk_usage: ProcessDiskUsage {
                read_bytes,
                total_read_bytes,
                total_written_bytes,
                written_bytes,
            },
        });
    }

    for disk in &disk_list {
        disks += format!("{disk:?}").as_str();
    }

    for (interface_name, data) in &networks {
        if data.total_received() > 1 {
            let interface_name = interface_name.to_string();
            network_interfaces.push(
                NetworkInterface { 
                    interface_name,
                    total_received: (data.total_received() as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0,
                    total_transmitted: (data.total_transmitted() as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0
                }
            );
        }
    }

    for component in &components {
        component_temps.insert(component.label().to_string(), component.temperature().unwrap_or_default());
        // comps += format!("{component:#?} \n", component.).as_str();
    }

    // let mut s = System::new_with_specifics(RefreshKind::everything());

    tokio::time::sleep(Duration::from_millis(200)).await;

    // s.refresh_cpu_all(); // Refreshing CPU information.
    for cpu in sys.cpus() {
        cpu_percentage = cpu.cpu_usage();
        cpu_clock = cpu.frequency() as f32;
    }

    Ok(SystemInformation {
        name,
        gpu_info,
        os_version,
        kernel_version,
        disks,
        total_memory,
        hostname,
        cpu_percentage,
        cpu_clock,
        component_temps,
        used_memory,
        number_of_cpus,
        network_interfaces,
        processes,
    })
}

// Function to generate client ID
pub fn generate_client_id(hostname: String, cpu: String) -> String {
    let cpu_id = env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown-cpu".to_string());
    let combined = format!("{}-{}-{}", hostname, cpu, cpu_id);
    info!("Filesystem -> generate_client_id -> combined: {}", combined.clone());
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let result = hasher.finalize();
    let hex_string = hex::encode(result);
    info!("Filesystem -> generate_client_id -> hex_string: {}", hex_string.clone());
    hex_string
}

pub async fn live_computer_stats(tx: Sender<SystemInformation>) -> anyhow::Result<(), anyhow::Error>{
    loop {
        tx.send(get_sysinfo().await?)?;
        tokio::time::sleep(std::time::Duration::from_secs_f32(0.1)).await;
    }
    #[allow(unreachable_code)]
    Ok(())
}

pub async fn get_sysinfo_no_gpu() -> anyhow::Result<SystemInformation, anyhow::Error> {
    let gpu_info = Gpu::default();    
    let sys = &mut SYSINFO.lock().await;
    // info!("GPU: {gpu_info:?}");
    sys.refresh_all();
    let mut cpu_percentage = f32::default();
    let mut cpu_clock = f32::default();
    let mut disks = String::new();
    let disk_list = Disks::new_with_refreshed_list();
    let mut network_interfaces: Vec<NetworkInterface> = Vec::new();
    let mut component_temps: HashMap<String, f32> = HashMap::new();
    let mut processes: Vec<SysProcess> = Vec::new();
    // Components temperature:
    let components = Components::new_with_refreshed_list();
    // Network interfaces name, total data received and total data transmitted:
    let networks = Networks::new_with_refreshed_list();
    // RAM and swap information:
    let total_memory = sys.total_memory() as f32 / (1024.0 * 1024.0);
    let used_memory = sys.used_memory() as f32 / (1024.0 * 1024.0);

    // Display system information:
    let name = System::name().context("Could not retrieve system name")?;
    let kernel_version = System::kernel_version().context("Could not retrieve kernel_version")?;
    let os_version = System::os_version().context("Could not retrieve os_version")?;
    let hostname = System::host_name().context("Could not retrieve hostname")?;

    // Number of CPUs:
    let number_of_cpus = format!("NB CPUs: {} \n", sys.cpus().len());
    
    // Display processes ID, name na disk usage:
    // for (pid, process) in sys.processes() {log::info!("[{pid}] {:?} {:?}", process.name(), process.disk_usage());}
    for (pid, process) in sys.processes().iter() {
        let id = pid.as_u32();
        let name = process.name().to_string_lossy().to_string();
        let cmd = format!("{:?}", process.cmd());
        let user_id = process.user_id().map(|id| id.to_string());
        
        let memory = (process.memory() as f32 / (1024.0 * 1024.0) * 100.0).round() / 100.0;

        let cpu_usage = process.cpu_usage();
        let read_bytes = (process.disk_usage().read_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
        let total_read_bytes = (process.disk_usage().total_read_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
        let total_written_bytes = (process.disk_usage().total_written_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;
        let written_bytes = (process.disk_usage().written_bytes as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0;

        processes.push(SysProcess {
            id,
            name,
            cmd,
            user_id,
            memory,
            cpu_usage,
            process_disk_usage: ProcessDiskUsage {
                read_bytes,
                total_read_bytes,
                total_written_bytes,
                written_bytes,
            },
        });
    }

    for disk in &disk_list {
        disks += format!("{disk:?}").as_str();
    }

    for (interface_name, data) in &networks {
        if data.total_received() > 1 {
            let interface_name = interface_name.to_string();
            network_interfaces.push(
                NetworkInterface { 
                    interface_name,
                    total_received: (data.total_received() as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0,
                    total_transmitted: (data.total_transmitted() as f32  / (1024.0 * 1024.0) * 100.0).round() / 100.0
                }
            );
        }
    }

    for component in components.list() {
        component_temps.insert(component.label().to_string(), component.temperature().unwrap_or_default());
        // comps += format!("{component:#?} \n", component.).as_str();
    }

    // let mut s = System::new_with_specifics(RefreshKind::everything());

    tokio::time::sleep(Duration::from_millis(200)).await;

    // s.refresh_cpu_all(); // Refreshing CPU information.
    for cpu in sys.cpus() {
        cpu_percentage = cpu.cpu_usage();
        cpu_clock = cpu.frequency() as f32;
    }

    Ok(SystemInformation {
        name,
        gpu_info,
        os_version,
        kernel_version,
        disks,
        total_memory,
        hostname,
        cpu_percentage,
        cpu_clock,
        component_temps,
        used_memory,
        number_of_cpus,
        network_interfaces,
        processes,
    })
}
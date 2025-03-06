use crate::tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram};
use powershell_script::PsScriptBuilder;
use sysinfo::Disks;
use walkdir::WalkDir;
use std::{collections::HashSet, path::{Path, PathBuf}};
use super::ScriptsTab;


impl <'a> ScriptsTab <'a> {
    pub fn _run_prechecks(&mut self) {
        self.log_message("Running system prechecks...");
    
        // Check Installed Programs
        if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
            for program in &mut *programs {
                if let Some(name) = &program.publisher {
                    match name.as_str() {
                        "OneLaunch" => match program.uninstall() {
                            Ok(_) => self.log_message("Uninstalled OneLaunch"),
                            Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                        }
                        "WebNavigatorBrowser" => {}
                        "ESET Security" => {}
                        //ccleaner browser, SAS browser extension
                        "Wavesor" => {}
                        "Clear Browser" => {}
                        "Shift Browser" => {}
                        "Avast Browser" => {}
                        "Mcaffee Safe Search" => {}
                        "Driver Support" => {}
                        "Winzip" => {}
                        "SuperAntiSpyware" => self.update_checklist("Informational", "Is SuperAntiSpyware installed?", true),
                        "Webroot" => self.update_checklist("Informational", "Is Webroot installed?", true),
                        _ => {}
                    }
                }
            }
            self.update_checklist("Informational", "Is SuperEasyBackup installed?", 
                programs.iter().any(|p| p.display_name.as_deref() == Some("SuperEasyBackup"))
            );
        }
    
        // Check Running Processes for Webroot and SAS
        if let Ok(processes) = _get_running_processes() {
            self.update_checklist("Informational", "Is Webroot Active?", 
                processes.contains("WRSA.exe")
            );
    
            self.update_checklist("Informational", "Is SuperAntiSpyware Active?", 
                processes.contains("SUPERANTISPYWARE.exe")
            );
        }
    
        // Check Antivirus Products
        if let Ok(av_products) = AntiVirusProduct::query_installed() {
            let active_avs: Vec<String> = av_products.iter()
                .filter(|av| av.decode_product_state().0)  // Check if AV is enabled
                .map(|av| av.display_name.clone())
                .collect();
            
            for av in av_products {
                self.log_message(&format!("AV: {av:#?}"));
            }
            self.update_checklist("Informational", "If Webroot/SAS not installed, what AV is active?", 
                !active_avs.is_empty()
            );
        }
    
        // Check Scheduled Tasks
        if let Ok(tasks) = ScheduledTask::list_tasks() {
            self.update_checklist("Informational", "Are there scheduled tasks for SuperAntiSpyware?", 
                tasks.iter().any(|t| t.task_name.as_deref() == Some("SuperAntiSpyware"))
            );
        }
    
        // Check Startup Apps
        if let Ok(startup_apps) = StartupProgram::get_startup_programs() {
            self.update_checklist("Actionable", "Disabling Startup Apps", 
                !startup_apps.is_empty()
            );
        }
    
        // Check System Settings
        self.update_checklist("Informational", "Are there any pending Windows updates?", 
            self.windows_updates.updates.len() > 0
        );
    
        self.update_checklist("Informational", "Is Windows Activated?", 
            _check_windows_activation()
        );
    
        // self.update_checklist("Informational", "Is Sleep enabled?", 
        //     check_sleep_mode()
        // );
    
        self.update_checklist("Informational", "Is Hibernation enabled?", 
            _check_hibernation_mode().unwrap_or(false)
        );
    
        self.log_message("Prechecks completed.");
    }
}

fn _get_running_processes() -> Result<HashSet<String>, anyhow::Error> {
    let ps_script = r#"Get-Process | Select-Object -ExpandProperty ProcessName | ConvertTo-Json"#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(ps_script)?;
    if output.success() {
        let stdout = output.stdout().unwrap_or_default();
        let processes: Vec<String> = serde_json::from_str(&stdout)?;
        Ok(processes.into_iter().collect())
    } else {
        Err(anyhow::anyhow!("Failed to retrieve running processes"))
    }
}

fn _install_pc_health_check() -> String {
    format!("winget install Microsoft.WindowsPCHealthCheck -h --accept-package-agreements --force")
}

fn _install_windbg() -> String {
    format!("winget install Microsoft.WinDbg -h --accept-package-agreements --force")
}

fn _check_program_running(process_name: &str) -> String {
    format!(
        r#"
        $process = Get-Process -Name "{}" -ErrorAction SilentlyContinue
        if ($process) {{ "Running" }} else {{ "Not Running" }}
        "#,
        process_name
    )
}

pub fn _check_windows_activation() -> bool {
    let script = r#"
        $status = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SoftwareProtectionPlatform").LicStatusArray
        if ($status -eq 1) { "Activated" } else { "Not Activated" }
    "#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(false)
        .print_commands(false)
        .build();

    match ps.run(script) {
        Ok(output) => output.stdout().unwrap_or_default().trim() == "Activated",
        Err(_) => false,  // Assume not activated if an error occurs
    }
}

fn _check_sleep_mode() -> &'static str {
    r#"
    $powercfg = powercfg -query | Select-String "HIBERNATE"
    if ($powercfg) { "Enabled" } else { "Disabled" }
    "#
}


fn _check_hibernation_mode() -> anyhow::Result<bool, anyhow::Error> {
    let ps_script = r#"powercfg -query | Select-String "HIBERNATE""#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(ps_script)?;
    Ok(!output.stdout().unwrap_or_default().trim().is_empty())
}


use windows::Storage::{UserDataPaths, SystemDataPaths};
pub fn get_data_transfer_candidates() -> anyhow::Result<Vec<(String, String)>, anyhow::Error> {
    let user_data: UserDataPaths = UserDataPaths::GetDefault()?;
    let sys_data = SystemDataPaths::GetDefault()?;

    log::info!(
        "User data: {:?}\n {:?}",
        user_data.Desktop()?,
        sys_data.UserProfiles()?
    );

    // user_data.
    let disks = Disks::new_with_refreshed_list();
    let mount_points = disks
        .iter()
        .map(|d| d.mount_point())
        .collect::<Vec<&Path>>();

    let mut paths_with_sizes = Vec::new();

    for drive in mount_points {
        let results = read_folder(
            &drive.to_path_buf(), 
            1, 
            true
        );
        if !results.is_empty() {
            for path in results {
                let dir_size = get_directory_size(path.as_path());
                let formatted_size = format_size(dir_size);
        
                log::info!("Directory: {:>10} | Size: {}", path.display(), formatted_size);
                paths_with_sizes.push((path.to_string_lossy().to_string(), formatted_size));
            }
        }
    }
    

    Ok(paths_with_sizes)
}

/// Get the total size of a directory (recursive) in bytes
fn get_directory_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|metadata| metadata.is_file()) // Only count file sizes
        .map(|metadata| metadata.len())
        .sum()
}

/// Convert bytes to human-readable MB/GB
fn format_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    }
}


pub fn read_folder(path: &PathBuf, depth: usize, read_dirs_only: bool) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = WalkDir::new(path)
        .min_depth(depth)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok()) // Only retrieve the resulted items
        .filter(|entry| !read_dirs_only || entry.path().is_dir()) // Include only directories if read_dirs_only is true
        .map(|entry| entry.path().to_path_buf()) // Iterate through each DirEntry
        .collect();

    // log::info!("path: {path:?} \nresult: {result:?}");
    result.sort_by(|a, b| {
        let da = a.is_dir();
        let db = b.is_dir();
        match da == db {
            true => a.file_name().cmp(&b.file_name()),
            false => db.cmp(&da),
        }
    });

    let result = result
        .into_iter()
        .filter(|path| {
            if read_dirs_only && !path.is_dir() {
                return false;
            }

            // log::info!("path.file_name(): {:?}", path.file_name());
            // Only include the "Users" directory if it exists in the path
            path.file_name()
                .map(|name| name == "Users")
                .unwrap_or(false)
        })
        .collect::<Vec<PathBuf>>();

    result
}



pub fn _activate_seb(activation_code: &str) -> anyhow::Result<(), anyhow::Error> {
    let _install_cmd = format!(r#"
        msiexec /i SuperEasyBackup.msi /qn Silent=1 ActivationURL=https://blue.mysecuredatavault.com ActivationCode={}
    "#, activation_code);
    Ok(())
}

pub fn _install_libre_office() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _find_activation_keys() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _prompt_for_user_pw() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _checkdisk() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _dism_scan() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _sfc_scan() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}
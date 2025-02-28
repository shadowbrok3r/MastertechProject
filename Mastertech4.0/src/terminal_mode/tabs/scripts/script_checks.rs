use crate::tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram};
use powershell_script::PsScriptBuilder;
use std::collections::HashSet;
use super::ScriptsTab;


impl <'a> ScriptsTab <'a> {
    pub fn run_prechecks(&mut self) {
        self.log_message("Running system prechecks...");
    
        // Check Installed Programs
        if let Ok(programs) = InstalledProgram::get_installed_programs() {
            self.update_checklist("Prechecks", "Is SuperEasyBackup installed?", 
                programs.iter().any(|p| p.display_name.as_deref() == Some("SuperEasyBackup"))
            );
    
            self.update_checklist("Prechecks", "Is Webroot installed?", 
                programs.iter().any(|p| p.display_name.as_deref() == Some("Webroot"))
            );
    
            self.update_checklist("Prechecks", "Is SuperAntiSpyware installed?", 
                programs.iter().any(|p| p.display_name.as_deref() == Some("SuperAntiSpyware"))
            );
        }
    
        // Check Running Processes for Webroot and SAS
        if let Ok(processes) = get_running_processes() {
            self.update_checklist("Prechecks", "Is Webroot Active?", 
                processes.contains("WRSA.exe")
            );
    
            self.update_checklist("Prechecks", "Is SuperAntiSpyware Active?", 
                processes.contains("SUPERANTISPYWARE.exe")
            );
        }
    
        // Check Antivirus Products
        if let Ok(av_products) = AntiVirusProduct::query_installed() {
            let active_avs: Vec<String> = av_products.iter()
                .filter(|av| av.decode_product_state().0)  // Check if AV is enabled
                .map(|av| av.display_name.clone())
                .collect();
            
            self.update_checklist("Prechecks", "If Webroot/SAS not installed, what AV is active?", 
                !active_avs.is_empty()
            );
        }
    
        // Check Scheduled Tasks
        if let Ok(tasks) = ScheduledTask::list_tasks() {
            self.update_checklist("Prechecks", "Are there scheduled tasks for SuperAntiSpyware?", 
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
        self.update_checklist("Prechecks", "Are there any pending Windows updates?", 
            self.windows_updates.updates.len() > 0
        );
    
        self.update_checklist("Prechecks", "Is Windows Activated?", 
            check_windows_activation()
        );
    
        // self.update_checklist("Prechecks", "Is Sleep enabled?", 
        //     check_sleep_mode()
        // );
    
        self.update_checklist("Prechecks", "Is Hibernation enabled?", 
            check_hibernation_mode().unwrap_or(false)
        );
    
        self.log_message("Prechecks completed.");
    }
}

fn get_running_processes() -> Result<HashSet<String>, anyhow::Error> {
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

pub fn check_windows_activation() -> bool {
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


fn check_hibernation_mode() -> anyhow::Result<bool, anyhow::Error> {
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

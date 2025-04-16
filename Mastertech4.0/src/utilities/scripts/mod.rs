pub mod task_scheduler;
pub mod taskbar;
pub mod startup;
pub mod processes;
#[cfg(target_os="windows")]
pub mod programs;
#[cfg(target_os="windows")]
pub mod antivirus;

use powershell_script::PsScriptBuilder;

pub use {
    task_scheduler::*,
    taskbar::*,
    startup::*,
    processes::*,
};

#[cfg(target_os="windows")]
pub use {
    programs::*,
    antivirus::*  
};

fn _install_pc_health_check() -> anyhow::Result<String, anyhow::Error> {
    Ok(run_ps_script("winget install Microsoft.WindowsPCHealthCheck -h --accept-package-agreements --force")?)
}

fn _install_windbg() -> anyhow::Result<String, anyhow::Error> {
    Ok(run_ps_script("winget install Microsoft.WinDbg -h --accept-package-agreements --force")?)
}

pub fn _find_activation_keys() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _prompt_for_user_pw() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _checkdisk() -> anyhow::Result<String, anyhow::Error> { Ok(run_ps_script("chkdsk /f/x/r C:")?) }

pub fn _dism_scan() -> anyhow::Result<String, anyhow::Error> { Ok(run_ps_script("")?) }

pub fn _sfc_scan() -> anyhow::Result<String, anyhow::Error> { Ok(run_ps_script("sfc /scannow")?) }

pub fn check_power_options() -> anyhow::Result<(), anyhow::Error> {

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(CHECK_POWER_OPTIONS)?;
    log::info!("output.stdout(): {:?}", output.stdout());
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Process {
    #[serde(rename="ProcessId")]
    pub id: usize,
    #[serde(rename="Name")]
    pub process_name: String,
    #[serde(rename="ExecutablePath")]
    pub exe_path: Option<String>,
}

pub fn get_running_processes() -> Result<Vec<Process>, anyhow::Error> {
    let ps_script = r#"Get-WmiObject Win32_Process | Select-Object ProcessId, Name, ExecutablePath | ConvertTo-Json"#; // r#"Get-Process | Select-Object Id, ProcessName | ConvertTo-Json"#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(ps_script)?;
    if output.success() {
        let stdout = output.stdout().unwrap_or_default();
        Ok(serde_json::from_str(&stdout)?)
    } else {
        Err(anyhow::anyhow!("Failed to retrieve running processes"))
    }
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

pub const CHECK_POWER_OPTIONS: &str = r#"
    # Define GUIDs and aliases for power settings
    $settings = @(
        @{
            Name = "Turn off display after"
            SubgroupGUID = "7516b95f-f776-4464-8c53-06167f40cc99"  # SUB_VIDEO
            SettingGUID = "3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e"   # VIDEOIDLE
            Units = "Seconds"
        },
        @{
            Name = "Sleep after"
            SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
            SettingGUID = "29f6c1db-86da-48c5-9fdb-f2b67b1f44da"   # STANDBYIDLE
            Units = "Seconds"
        },
        @{
            Name = "Allow hybrid sleep"
            SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
            SettingGUID = "94ac6d29-73ce-41a6-809f-6363ba21b47e"   # HYBRIDSLEEP
            Units = "On/Off"
        },
        @{
            Name = "Hibernate after"
            SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
            SettingGUID = "9d7815a6-7ee4-497e-8888-515a05f02364"   # HIBERNATEIDLE
            Units = "Seconds"
        }
    )

    # Function to convert hex value to decimal (for seconds) or interpret as On/Off
    function Convert-PowerSettingValue {
        param (
            [string]$HexValue,
            [string]$Units
        )
        if ($Units -eq "Seconds") {
            [uint32]("0x" + $HexValue)
        } elseif ($Units -eq "On/Off") {
            if ($HexValue -eq "0x00000000") { "Off" } else { "On" }
        }
    }

    # Function to check if any setting is enabled
    function Check-PowerSettingsEnabled {
        $anyEnabled = $false
        foreach ($setting in $settings) {
            # Query current AC and DC settings
            $acResult = powercfg /query SCHEME_CURRENT $setting.SubgroupGUID $setting.SettingGUID | Select-String "Current AC Power Setting Index: (0x[0-9a-fA-F]+)"
            $dcResult = powercfg /query SCHEME_CURRENT $setting.SubgroupGUID $setting.SettingGUID | Select-String "Current DC Power Setting Index: (0x[0-9a-fA-F]+)"
            
            $acValue = if ($acResult) { $acResult.Matches.Groups[1].Value } else { "0x00000000" }
            $dcValue = if ($dcResult) { $dcResult.Matches.Groups[1].Value } else { "0x00000000" }
            
            $acConverted = Convert-PowerSettingValue -HexValue $acValue -Units $setting.Units
            $dcConverted = Convert-PowerSettingValue -HexValue $dcValue -Units $setting.Units

            Write-Host "$($setting.Name): AC = $acConverted, DC = $dcConverted"

            # Check if enabled (non-zero for Seconds, "On" for On/Off)
            if (($setting.Units -eq "Seconds" -and ($acConverted -gt 0 -or $dcConverted -gt 0)) -or 
                ($setting.Units -eq "On/Off" -and ($acConverted -eq "On" -or $dcConverted -eq "On"))) {
                $anyEnabled = $true
            }
        }
        return $anyEnabled
    }


    # Main logic
    Write-Host "Checking power settings..."
    Write-output Check-PowerSettingsEnabled
"#;
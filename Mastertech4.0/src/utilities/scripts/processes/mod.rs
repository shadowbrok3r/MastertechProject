use powershell_script::PsScriptBuilder;
use serde::{Deserialize, Serialize};

/// Represents a process creation event.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessCreationEvent {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "ProcessId")]
    pub process_id: u32,

    #[serde(rename = "CommandLine")]
    pub command_line: Option<String>,
}

impl ProcessCreationEvent {
    /// Monitors process creation events using PowerShell.
    /// Monitors process creation events using PowerShell.
    pub fn monitor_process_creation() -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let ps_script = r#"
        $query = "SELECT * FROM __InstanceCreationEvent WITHIN 1 WHERE TargetInstance ISA 'Win32_Process'"
        $outputFile = Join-Path -Path $env:userprofile\DESKTOP -ChildPath "process_events.json"


        if (Test-Path $outputFile) {
            Remove-Item $outputFile
        }

        Register-WmiEvent -Namespace "root\CIMv2" -Query $query -Action {
            $process = $Event.SourceEventArgs.NewEvent.TargetInstance
            [pscustomobject]@{
                Name        = $process.Name
                ProcessId   = $process.ProcessId
                CommandLine = $process.CommandLine
            } | ConvertTo-Json  | Add-Content -Path $outputFile -Encoding UTF8
        }

        Write-Host "Monitoring process creation events. Press Ctrl+C to stop."
        Start-Sleep -Seconds 10
        Unregister-Event -SourceIdentifier $query
        Remove-Event -SourceIdentifier $query
        Write-Output $outputFile
        "#;

        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        log::info!("Starting process monitoring...");

        let output = ps.run(ps_script)?;

        if output.success() {
            let stdout = output.stdout().unwrap_or_default();
            let events_file = stdout.trim();

            // Read and parse the JSON file
            let content = std::fs::read_to_string(events_file)?;
            let events: Vec<ProcessCreationEvent> = serde_json::from_str(&content)?;
            Ok(events)
        } else {
            Err(anyhow::anyhow!(format!(
                "Failed to monitor process creation: {}",
                output.stderr().unwrap_or_default()
            )))
        }
    }
}

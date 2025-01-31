/// Struct for generic installed programs
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InstalledProgram {
    #[serde(rename = "DisplayName")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub display_name: Option<String>,

    #[serde(rename = "DisplayVersion")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub display_version: Option<String>,

    #[serde(rename = "Publisher")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub publisher: Option<String>,

    #[serde(rename = "UninstallString")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub uninstall_string: Option<String>,

    #[serde(rename = "InstallLocation")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub install_location: Option<String>,

    #[serde(rename = "InstallDate")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub install_date: Option<String>,

    #[serde(rename = "PSPath")]
    #[serde(deserialize_with = "deserialize_string_or_map")]
    pub ps_path: Option<String>,
}

impl InstalledProgram {
    /// Retrieves all installed general programs
    pub fn get_installed_programs() -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let script = r#"
            $programs = @()

            # Query installed programs from both HKLM and HKCU
            $paths = @(
                "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
            )

            foreach ($path in $paths) {
                if (Test-Path $path) {
                    $programs += Get-ItemProperty -Path $path |
                        Select-Object DisplayName, DisplayVersion, Publisher, UninstallString, InstallLocation, InstallDate, PSPath
                }
            }

            # Convert to JSON and return
            $programs | ConvertTo-Json
        "#;

        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(false)
            .print_commands(true)
            .build();

        let output = ps.run(script)?;

        if output.success() {
            let stdout = output.stdout().unwrap_or_default();

            // Deserialize JSON into a Vec<GeneralProgram>
            match serde_json::from_str::<Vec<Self>>(&stdout) {
                Ok(programs) => Ok(programs),
                Err(_) => {
                    // Handle case where JSON is a single object
                    let single_program: Self = serde_json::from_str(&stdout)?;
                    Ok(vec![single_program])
                }
            }
        } else {
            Err(anyhow::anyhow!(format!(
                "Failed to retrieve installed programs: {}",
                output.stderr().unwrap_or_default()
            )))
        }
    }

    /// Retrieves an installed program by name.
    pub fn get_by_name(program_name: &str) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(false)
            .print_commands(true)
            .build();

        let script = format!(
            r#"
            Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" |
            Where-Object {{ ($_.DisplayName -match "{program_name}" -or $_.Publisher -match "{program_name}") }} |
            Select-Object DisplayName, DisplayVersion, Publisher, UninstallString, InstallLocation, InstallDate, PSPath |
            ConvertTo-Json
            "#,
            program_name = program_name
        );

        let output = ps.run(&script)?;

        if output.success() {
            let stdout = output.stdout().unwrap_or_default();

            match serde_json::from_str::<Self>(&stdout) {
                Ok(program) => Ok(Some(program)),
                Err(_) => Ok(None),
            }
        } else {
            Err(anyhow::anyhow!(format!(
                "Failed to query installed programs: {}",
                output.stderr().unwrap_or_default()
            )))
        }
    }

    pub fn uninstall(&self) -> anyhow::Result<(), anyhow::Error> {
        if let (Some(command), Some(display_name)) = (&self.uninstall_string, &self.display_name) {

            // try this for inno setups
            let script = format!(r#"& {command} /verysilent /suppressmsgboxes /norestart"#);
            log::info!("Uninstalling program: {display_name}\nUninstall String: {command}\n{script}");
            let ps = powershell_script::PsScriptBuilder::new()
                .no_profile(true)
                .non_interactive(true)
                .hidden(false)
                .print_commands(true)
                .build();

            let output = ps.run(&script)?;

            if output.success() {
                log::info!("Successfully uninstalled: {}", display_name);
                Ok(())
            } else {
                Err(anyhow::anyhow!(format!(
                    "Failed to uninstall {:?}: {}",
                    display_name,
                    output.stderr().unwrap_or_default()
                )))
            }
        } else {
            Err(anyhow::anyhow!(format!(
                "No uninstall string found for program: {:?}",
                self.display_name
            )))
        }
    }
}

/// Custom deserializer to handle fields that may be a string or a map
fn deserialize_string_or_map<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => Ok(Some(s)),
        serde_json::Value::Object(_) => Ok(None), // If it's a map, return None
        _ => Ok(None), // For other cases, also return None
    }
}

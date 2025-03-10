use powershell_script::PsScriptBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct StartupProgram {
    #[serde(rename = "Path")]
    pub path: String,

    #[serde(rename = "KeyName")]
    pub key_name: String,

    #[serde(rename = "PropertyName")]
    pub property_name: String,
    /// Handles dynamic types
    #[serde(rename = "Value")]
    pub value: Value,
    /// Decoded state for `StartupApproved\Run` entries
    #[serde(rename = "DecodedState")]
    pub decoded_state: Option<StartupState>, 
}


/// Represents the state of a startup item.
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StartupState {
    #[default]
    Enabled,
    Disabled,
    DisabledByUser,
    UnknownState,
}


impl StartupProgram {
    /// Retrieves all startup programs by querying specific registry paths.
    pub fn get_startup_programs() -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let ps_script = r#"
        $paths = @(
            "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run",
            "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run",
            "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
            "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
        )

        $results = @()

        foreach ($path in $paths) {
            if (Test-Path $path) {
                Get-ItemProperty -Path $path |
                ForEach-Object {
                    $keyName = $_.PSChildName
                    $propertyNames = $_ | Get-Member -MemberType NoteProperty | Select-Object -ExpandProperty Name

                    foreach ($propertyName in $propertyNames) {
                        $value = $_.$propertyName
                        $decodedState = $null

                        if ($path -like "*StartupApproved*") {
                            if ($value -is [byte[]] -and $value.Length -ge 1) {
                                switch ($value[0]) {
                                    0x02 { $decodedState = "enabled" }
                                    0x03 { $decodedState = "disabled" }
                                    0x06 { $decodedState = "disabled_by_user" }
                                    default { $decodedState = "unknown_state" }
                                }
                            }
                        }

                        $results += [pscustomobject]@{
                            Path          = $path
                            KeyName       = $keyName
                            PropertyName  = $propertyName
                            Value         = $value
                            DecodedState  = $decodedState
                        }
                    }
                }
            }
        }

        $results | ConvertTo-Json
        "#;

        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        let output = ps.run(ps_script)?;

        if output.success() {
            let stdout = output.stdout().unwrap_or_default();
            let startup_programs: Vec<StartupProgram> = serde_json::from_str(&stdout)?;
            Ok(startup_programs)
        } else {
            Err(anyhow::anyhow!(format!(
                "Failed to retrieve startup programs: {}",
                output.stderr().unwrap_or_default()
            )))
        }
    }

    /// Decodes the `Value` field if it's a byte array.
    /// Dynamically decode the startup state from the `Value` field.
    pub fn decode_value(&self) -> Option<StartupState> {
        if let Value::Array(bytes) = &self.value {
            if let Some(first_byte) = bytes.get(0).and_then(|v| v.as_u64()) {
                match first_byte {
                    0x02 => Some(StartupState::Enabled),
                    0x03 => Some(StartupState::Disabled),
                    0x06 => Some(StartupState::DisabledByUser),
                    _ => Some(StartupState::UnknownState),
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

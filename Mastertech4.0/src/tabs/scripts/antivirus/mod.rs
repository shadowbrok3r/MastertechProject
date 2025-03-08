use powershell_script::PsScriptBuilder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AntiVirusProduct {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "instanceGuid")]
    pub instance_guid: String,
    #[serde(rename = "pathToSignedProductExe")]
    pub path_to_signed_product_exe: String,
    #[serde(rename = "pathToSignedReportingExe")]
    pub path_to_signed_reporting_exe: String,
    #[serde(rename = "productState")]
    pub product_state: u32,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "PSComputerName")]
    pub ps_computer_name: Option<String>,
}

impl AntiVirusProduct {
    /// Queries all installed antivirus products using PowerShell.
    pub fn query_installed() -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        let script = r#"
        Get-CimInstance -Namespace "Root\SecurityCenter2" -ClassName AntiVirusProduct | ConvertTo-Json
        "#;

        let output = ps.run(script)?;

        if output.success() {
            let stdout = output.stdout().unwrap_or_default();

            // Try to deserialize as an array (sequence)
            match serde_json::from_str::<Vec<Self>>(&stdout) {
                Ok(products) => Ok(products),
                Err(_) => {
                    // If deserialization as an array fails, try as a single object (map)
                    let single_product: Self = serde_json::from_str(&stdout)?;
                    Ok(vec![single_product])
                }
            }
        } else {
            Err(anyhow::anyhow!(output.stderr().unwrap_or_else(|| "Unknown error".to_string())))
        }
    }

    /// Decodes the `productState` bitmask into human-readable components.
    pub fn decode_product_state(&self) -> (bool, bool, bool) {
        let enabled = (self.product_state & 0x10000) != 0;
        let real_time_protection = (self.product_state & 0x20000) != 0;
        let signatures_up_to_date = (self.product_state & 0x40000) != 0;

        (enabled, real_time_protection, signatures_up_to_date)
    }

    /// Uninstalls the antivirus product using the `instanceGuid`.
    pub async fn uninstall(&self) -> anyhow::Result<(), anyhow::Error> {
        let script = format!(
            r#"
            $guid = "{instance_guid}"
            Get-CimInstance -Namespace "ROOT\SecurityCenter2" -ClassName AntiVirusProduct | Where-Object {{ $_.instanceGuid -eq $guid }} | ForEach-Object {{
                Write-Output "Uninstalling $($_.displayName)..."
                # Assuming a hypothetical uninstaller command
                & "msiexec.exe" /x $($_.instanceGuid) /quiet
            }}
            "#,
            instance_guid = self.instance_guid
        );

        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        let output = ps.run(&script)?;
        if output.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(format!(
                "Failed to uninstall {}: {}",
                self.display_name,
                output.stderr().unwrap_or_default()
            )))
        }
    }
}

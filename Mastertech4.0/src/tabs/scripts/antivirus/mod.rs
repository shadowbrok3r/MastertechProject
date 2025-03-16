use database::schema::{find_latest_carbonite_entry, CarboniteResponse};
use tokio::{fs, io::AsyncWriteExt, process::Command};
use winapi::um::winbase::CREATE_NO_WINDOW;
use powershell_script::PsScriptBuilder;
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use futures::StreamExt;
use reqwest::Client;
use sha2::Digest;
use log::info;
use std::{io, path::PathBuf};

use super::InstalledProgram;

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


pub async fn install_webroot(
    activation_key: String, 
    client: Client,
    progress_tx: Sender<(u64, u64)>
) -> anyhow::Result<(), anyhow::Error> {
    if activation_key.is_empty() {
        return Err(anyhow::anyhow!("Activation key is empty"));
    }

    info!("running install_webroot!");
    let response = client
        .get("https://anywhere.webrootcloudav.com/zerol/wsainstall.exe")
        .send()
        .await?;

    let total_length = response.content_length().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "Content-Length header is missing")
    })?;

    let mut downloaded_bytes: u64 = 0;

    let temp_directory = std::env::temp_dir();
    let wrv_path = format!("{}\\wrv.exe", temp_directory.display());

    let mut file = fs::File::create(wrv_path.clone()).await?;
    let mut sha = sha2::Sha256::new();

    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        sha.update(&chunk);
        downloaded_bytes += chunk.len() as u64;
        progress_tx.try_send((downloaded_bytes, total_length))?;
    }

    if downloaded_bytes == total_length {
        let hash = sha.finalize();
        info!("Download complete. SHA-256: {:x}", hash);
        #[cfg(target_os = "windows")]
        {
            let cmd_stdout = Command::new("cmd")
                .arg("/c ")
                .arg(wrv_path)
                .arg(format!("/key={activation_key}"))
                .arg("/silent")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?
                .stdout;

            info!("cmd_stdout: {:?}", cmd_stdout);
        }
    }
    Ok(())
}

pub async fn install_sas(
    activation_key: String, 
    client: Client,
    progress_tx: Sender<(u64, u64)>
) -> anyhow::Result<(), anyhow::Error> {
    if activation_key.is_empty() {
        return Err(anyhow::anyhow!("Activation key is empty"));
    }
    if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
        for program in &mut *programs {
            if let (Some(publisher), Some(install_location)) = (&program.publisher, &program.install_location) {
                if program.display_name.clone().unwrap_or_default().contains("SUPERAntiSpyware")
                    || publisher.clone().contains("SUPERAntiSpyware")
                { // "C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe" /autoregister:1HT2-ZJEA-VV0B5
                    let path = PathBuf::from(install_location);
                    let sas_exe = path.join("SUPERAntiSpyware.exe");
                    if sas_exe.exists() {
                        log::info!("SAS EXE: cmd /c {sas_exe:?} /autoregister:{activation_key}");

                        let cmd_stdout = Command::new("cmd")
                            .arg("/c ")
                            .arg(sas_exe)
                            .arg(format!(" /autoregister:{activation_key}"))
                            .creation_flags(CREATE_NO_WINDOW)
                            .spawn()?
                            .stdout;
                        log::info!("cmd_stdout: {cmd_stdout:?}");
                        return Ok(());

                    } else {log::info!("Install location: {sas_exe:?}");}
                }
            }
        }
    }

    let response = client
        .get(format!(
            "https://secure.superantispyware.com/SUPERAntiSpyware.exe"
        ))
        .send()
        .await?;

    let total_length = response.content_length().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "Content-Length header is missing")
    })?;
    let mut downloaded_bytes: u64 = 0;

    let temp_directory = std::env::temp_dir();
    let sas_path = format!("{}\\sas.exe", temp_directory.display());

    let mut file = fs::File::create(sas_path.clone()).await?;
    let mut sha = sha2::Sha256::new();

    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        sha.update(&chunk);
        downloaded_bytes += chunk.len() as u64;
        progress_tx.try_send((downloaded_bytes, total_length))?;
    }

    if downloaded_bytes == total_length {

        let hash = sha.finalize();
        info!("Download complete. SHA-256: {:x}", hash);
        #[cfg(target_os = "windows")]
        {
            let cmd_stdout = Command::new("cmd")
                .arg("/c ")
                .arg(sas_path)
                .arg(format!("/REGCODE={activation_key}"))
                .arg("/silent")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?
                .stdout;

            info!("cmd_stdout: {:?}", cmd_stdout);
        }
    }
    Ok(())
}


pub async fn install_supereasybackup(
    customer_email: String, 
    client: Client,
    progress_tx: Sender<(u64, u64)>,
) -> anyhow::Result<(), anyhow::Error> {
    info!("running install_supereasybackup!");
    let response = client
        .get("https://dcgeneral.blob.core.windows.net/downloads/MUS/v11.2.0/DCProtect-11.2.0.5777-SuperEasyBackup.msi")
        .send()
        .await?;

    let total_length = response.content_length().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "Content-Length header is missing")
    })?;

    let mut downloaded_bytes: u64 = 0;

    let temp_directory = std::env::temp_dir();
    let seb_path = format!("{}\\seb.msi", temp_directory.display());

    let mut file = fs::File::create(seb_path.clone()).await?;
    let mut sha = sha2::Sha256::new();

    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        sha.update(&chunk);
        downloaded_bytes += chunk.len() as u64;
        progress_tx.try_send((downloaded_bytes, total_length))?;
    }

    if downloaded_bytes == total_length {
        let response_json: Vec<CarboniteResponse> = CarboniteResponse::default().from_customer_email(customer_email, client.clone()).await?;
        let hash = sha.finalize();
        info!("Download complete. SHA-256: {:x}", hash);

        if response_json.is_empty() { return Err(anyhow::anyhow!("Response is empty")); }

        if let Some(carbonite_entry) = find_latest_carbonite_entry(&response_json) {
            let activation_code = &carbonite_entry.activation_code;
            #[cfg(target_os = "windows")]
            {
                // msiexec /i SuperEasyBackup.msi /qn Silent=1 ActivationURL=https://blue.mysecuredatavault.com ActivationCode={}
                let cmd_stdout = Command::new("msiexec")
                    .arg("/i ")
                    .arg(seb_path)
                    .arg("/qn")
                    .arg("Silent=1")
                    .arg("ActivationURL=https://blue.mysecuredatavault.com")
                    .arg(format!("ActivationCode={}", activation_code))
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()?
                    .stdout;
    
                info!("cmd_stdout: {:?}", cmd_stdout);
            }
        }
    }
    Ok(())
}
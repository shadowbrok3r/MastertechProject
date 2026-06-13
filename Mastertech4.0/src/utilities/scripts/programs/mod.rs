use std::io;

use crossbeam::channel::Sender;
use reqwest::Client;
use tokio::{fs, io::AsyncWriteExt, process::Command};
use winapi::um::winbase::CREATE_NO_WINDOW;

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
            $paths = @(
                "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
            )

            foreach ($path in $paths) {
                if (Test-Path $path) {
                    $programs += Get-ItemProperty -Path $path |
                        Select-Object DisplayName, DisplayVersion, Publisher, UninstallString, InstallLocation, InstallDate, PSPath
                }
            }
            $programs | ConvertTo-Json
        "#;

        let ps = powershell_script::PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();

        let output = ps.run(script)?;

        if output.success() {
            let stdout = output.stdout().unwrap_or_default();

            // Deserialize JSON into a Vec<GeneralProgram>
            match serde_json::from_str::<Vec<Self>>(&stdout) {
                Ok(programs) => {
                    // Helper closure to process Option<String> -> Option<String>, removing ALL null bytes from inner String
                    let process_option_field = |opt_s: &Option<String>| -> Option<String> {
                        // .as_ref() borrows the content of the Option without moving it.
                        // .map() applies the closure if the Option is Some.
                        // s.replace('\0', "") creates a new String with nulls removed.
                        // The .map() automatically wraps the new String in Some(...).
                        // If opt_s was None, .map() does nothing and returns None.
                        
                        opt_s
                            .as_ref()
                            .map(|s| {
                                if s.contains('\0') {
                                    log::info!("Removed Null byte: {s:?}");
                                } else {
                                    // log::info!("No Nulls: {}", s);
                                }
                                s.replace('\0', "")

                                // s.chars()
                                //     .filter(|&c| {
                                //         // KEEP if: Not Replacement Char AND (Is Tab OR Is Not Control Char)
                                //         c != '\u{FFFD}' && (c == '\t' || !c.is_control())
                                //     })
                                //     .collect::<String>()
                            }).filter(|s| !s.is_empty())
                    };

                    let processed_programs: Vec<InstalledProgram> = programs
                        .iter() // Iterate over references to the original programs
                        .map(|program| {
                            // Create a *new* InstalledProgram instance for the results
                            InstalledProgram {
                                display_name: process_option_field(&program.display_name),
                                display_version: process_option_field(&program.display_version),
                                publisher: process_option_field(&program.publisher),
                                uninstall_string: process_option_field(&program.uninstall_string),
                                install_location: process_option_field(&program.install_location),
                                install_date: process_option_field(&program.install_date),
                                ps_path: process_option_field(&program.ps_path),
                            }
                        })
                        .collect(); // Collect the new instances into a Vec

                    Ok(processed_programs) // Return the new Vec wrapped in Ok
                },
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
            .hidden(true)
            .print_commands(false)
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
        // https://download.eset.com/com/eset/tools/installers/av_remover/latest/avremover_nt64_enu.exe
        if let (Some(command), Some(display_name)) = (self.uninstall_string.clone(), self.display_name.clone()) {    
            std::thread::spawn(move || {
                // try this for inno setups
                let script = format!(r#"& {command} /silent /verysilent /suppressmsgboxes /norestart"#);
                log::info!("Uninstalling program: {display_name}\nUninstall String: {command}\n{script}");
                let ps = powershell_script::PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();

                let output = ps.run(&script);
                
                if let Err(e) = output {
                    log::error!("Error with PS: {e:?}");
                    let cmd = command.to_lowercase();
                    if cmd.contains("MsiExec /I") 
                        || cmd.contains("--uninstall")
                        || cmd.contains("-uninstall") 
                    {

                    }
                    let ps = powershell_script::PsScriptBuilder::new()
                        .no_profile(true)
                        .non_interactive(true)
                        .hidden(true)
                        .print_commands(false)
                        .build();

                    let _output = ps.run(&format!("& {command}"));
                }
            });    
        }
        Ok(())
    }
}

pub async fn install_program(
    url: String, 
    client: Client,
    progress_tx: Sender<(u64, u64)>,
) -> anyhow::Result<(), anyhow::Error> {
    log::info!("running install_program!");

    let temp_directory = std::env::temp_dir();
    let download_path = format!("{}\\prgrm.exe", temp_directory.display());

    let need_download = match tokio::fs::metadata(&download_path).await {
        Ok(meta) if meta.len() > 500_000 => {
            log::info!("Cached installer found ({} bytes), trying it first", meta.len());
            false
        }
        _ => true,
    };

    if need_download {
        if let Err(e) = download_to_file(&client, &url, &download_path, &progress_tx).await {
            log::info!("Download failed ({e}), checking connectivity...");
            crate::utilities::windows::net_adapter::ensure_internet_connected().await?;
            download_to_file(&client, &url, &download_path, &progress_tx).await?;
        }
    }

    #[cfg(target_os = "windows")]
    {
        log::info!("Running installer (waiting for completion)...");
        let output = Command::new("cmd")
            .arg("/C")
            .arg(&download_path)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await?;

        log::info!("Installer exit status: {:?}", output.status);

        if !output.status.success() {
            log::info!("Cached installer failed, re-downloading...");
            let _ = tokio::fs::remove_file(&download_path).await;
            download_to_file(&client, &url, &download_path, &progress_tx).await?;

            let retry = Command::new("cmd")
                .arg("/C")
                .arg(&download_path)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await?;
            log::info!("Installer retry exit status: {:?}", retry.status);
        }
    }
    Ok(())
}

async fn download_to_file(
    client: &Client,
    url: &str,
    dest_path: &str,
    progress_tx: &Sender<(u64, u64)>,
) -> anyhow::Result<()> {
    use futures::StreamExt;
    use sha2::Digest;

    let response = client.get(url).send().await?;
    let total_length = response.content_length().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "Content-Length header is missing")
    })?;

    let mut downloaded_bytes: u64 = 0;
    let mut file = fs::File::create(dest_path).await?;
    let mut sha = sha2::Sha256::new();
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        sha.update(&chunk);
        downloaded_bytes += chunk.len() as u64;
        let _ = progress_tx.try_send((downloaded_bytes, total_length));
    }

    if downloaded_bytes != total_length {
        return Err(anyhow::anyhow!(
            "Incomplete download: got {downloaded_bytes} of {total_length} bytes"
        ));
    }

    let hash = sha.finalize();
    log::info!("Download complete ({dest_path}). SHA-256: {:x}", hash);
    Ok(())
}


pub fn run_ps_script(script: &str) -> anyhow::Result<String, anyhow::Error> {
    let ps = powershell_script::PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(&format!("& {script}"))?;
    let out = output.stdout().unwrap_or_default();
    Ok(out)
}

/// Removes the Copilot Store apps for the current user. On current Win11
/// builds the taskbar Copilot icon is this pinned app, not the legacy
/// ShowCopilotButton sidebar, so registry tweaks alone no longer unpin it.
pub fn remove_copilot_appx() -> anyhow::Result<Vec<String>> {
    let ps = powershell_script::PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let script = r#"
        foreach ($name in @('Microsoft.Copilot', 'Microsoft.Windows.Ai.Copilot.Provider')) {
            $pkg = Get-AppxPackage -Name $name -ErrorAction SilentlyContinue
            if ($pkg) {
                $pkg | Remove-AppxPackage -ErrorAction SilentlyContinue
                if (Get-AppxPackage -Name $name -ErrorAction SilentlyContinue) {
                    "FAILED to remove $name"
                } else {
                    "Removed $name"
                }
            } else {
                "$name not present"
            }
        }
    "#;

    let output = ps.run(script)?;
    let stdout = output.stdout().unwrap_or_default();
    let lines: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.iter().any(|l| l.starts_with("FAILED")) {
        return Err(anyhow::anyhow!(lines.join("; ")));
    }
    Ok(lines)
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

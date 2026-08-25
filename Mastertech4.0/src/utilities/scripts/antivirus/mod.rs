pub mod sas_tasks;

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
use std::{io, path::PathBuf, time::Duration};

use super::{get_running_processes, InstalledProgram};

/// Kills all running SUPERAntiSpyware processes (SUPERAntiSpyware.exe, SASCore, SASTask, etc).
/// Returns the number of processes killed.
pub fn kill_sas_processes() -> u32 {
    let mut killed = 0;
    if let Ok(processes) = get_running_processes() {
        for process in processes {
            let name = process.process_name.to_lowercase();
            let exe_path = process.exe_path.clone().unwrap_or_default().to_lowercase();
            if name.contains("sascore")
                || name.contains("sastask")
                || exe_path.contains("superanti")
                || name.contains("superanti")
            {
                log::info!("Killing SAS process PID {} ({})", process.id, process.process_name);
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &process.id.to_string(), "/F"])
                    .output();
                killed += 1;
            }
        }
    }
    killed
}

/// Starts the SUPERAntiSpyware tray application.
pub fn launch_sas_tray() -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    const SAS_EXE: &str = r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe";
    if !std::path::Path::new(SAS_EXE).exists() {
        return Err(anyhow::anyhow!("SUPERAntiSpyware is not installed"));
    }
    std::process::Command::new(SAS_EXE)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    log::info!("Launched {SAS_EXE}");
    Ok(())
}

/// Title of the top-level window SAS carries once a Professional key is bound,
/// or `None` when no such window exists.
///
/// The window is not visible and `MainWindowTitle` reads empty, so this needs an
/// enumeration rather than a process-property read. A string scan of
/// SAS_ALLUSER.DB3 is not usable instead: it is a single-page SQLite file that
/// retains superseded pages, so it reports `InstallType FREE` alongside a valid
/// `RegCodeEx` even when the product is fully activated.
fn sas_pro_window_title() -> Option<String> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe extern "system" fn scan(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = unsafe { &mut *(lparam.0 as *mut Option<String>) };
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len > 0 {
            let mut buf = vec![0u16; len as usize + 1];
            let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if title.to_lowercase().contains("superantispyware professional") {
                *found = Some(title);
                // Stop enumerating.
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    let mut found: Option<String> = None;
    let _ = unsafe {
        EnumWindows(
            Some(scan),
            LPARAM(&mut found as *mut Option<String> as isize),
        )
    };
    found
}

/// [`sas_pro_window_title`] off the runtime, since `GetWindowTextW` on another
/// process's window blocks until that process's message loop answers.
///
/// A probe that fails reads as "not activated yet", so the caller times out with
/// a real error rather than reporting an unproven success.
async fn sas_pro_window_title_async() -> Option<String> {
    tokio::task::spawn_blocking(sas_pro_window_title)
        .await
        .unwrap_or_default()
}

/// Kills `pid` and every descendant.
///
/// SAS's termination protection ignores a kill aimed at the
/// SUPERAntiSpyware.exe child and ignores `WM_CLOSE`; a tree kill of the
/// cmd.exe wrapper takes it.
fn taskkill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;

    let output = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(out) => log::info!("taskkill /PID {pid} /T /F: {:?}", out.status),
        Err(e) => log::info!("taskkill /PID {pid} /T /F failed: {e}"),
    }
}

/// Starts SAS's own Quick Scan scheduled task via `schtasks /Run`.
/// Reads the QUICK_SCAN task GUID from SAS_CURRENTUSER.DB3, configuring the
/// SAS settings + tasks first when none exist yet.
pub fn run_sas_quick_scan() -> anyhow::Result<Vec<String>> {
    use std::os::windows::process::CommandExt;

    const SAS_EXE: &str = r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe";
    if !std::path::Path::new(SAS_EXE).exists() {
        return Err(anyhow::anyhow!("SUPERAntiSpyware is not installed"));
    }

    let mut messages = Vec::new();
    let scan_guid = match sas_tasks::get_quick_scan_task_guid() {
        Ok(Some(guid)) => guid,
        _ => {
            messages.push("No SAS quick-scan task found; configuring SAS scheduled tasks...".to_string());
            let killed = kill_sas_processes();
            messages.push(format!("Killed {killed} SAS processes before configuring"));
            std::thread::sleep(Duration::from_secs(2));
            let (_, scan_guid) = sas_tasks::configure_sas_scheduled_tasks()?;
            scan_guid
        }
    };

    let task_name = format!(r"\SUPERAntiSpyware\SUPERAntiSpyware Scheduled Task {scan_guid}");
    let run_task = |name: &str| -> anyhow::Result<std::process::Output> {
        Ok(std::process::Command::new("schtasks")
            .args(["/Run", "/TN", name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?)
    };

    let mut output = run_task(&task_name)?;
    if !output.status.success() {
        // DB row exists but the Windows task is gone — re-register and retry once.
        messages.push("SAS scan task missing from Task Scheduler; re-registering...".to_string());
        let killed = kill_sas_processes();
        messages.push(format!("Killed {killed} SAS processes before configuring"));
        std::thread::sleep(Duration::from_secs(2));
        let (_, scan_guid) = sas_tasks::configure_sas_scheduled_tasks()?;
        let task_name = format!(r"\SUPERAntiSpyware\SUPERAntiSpyware Scheduled Task {scan_guid}");
        output = run_task(&task_name)?;
    }

    if output.status.success() {
        messages.push(format!("Started SAS quick scan (task {scan_guid})"));
        Ok(messages)
    } else {
        Err(anyhow::anyhow!(
            "schtasks /Run failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Launches a Webroot scan of C: using the documented `WRSA.exe -scan="C:"` switch.
pub fn start_webroot_scan() -> anyhow::Result<String> {
    use std::os::windows::process::CommandExt;

    let candidates = [
        r"C:\Program Files\Webroot\WRSA.exe",
        r"C:\Program Files (x86)\Webroot\WRSA.exe",
    ];
    let Some(exe) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
        return Err(anyhow::anyhow!("Webroot (WRSA.exe) is not installed"));
    };

    std::process::Command::new(exe)
        .raw_arg(r#"-scan="C:""#)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(format!("Started Webroot scan: {exe} -scan=\"C:\""))
}

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


/// What `install_webroot` actually did. Exit status alone cannot tell these
/// apart: a keyed installer run over a live agent of the same version exits 0
/// in seconds without binding the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebrootInstallOutcome {
    FreshInstall,
    Upgraded,
    /// Keycode bound to an existing install without the binaries changing.
    ReKeyed,
    /// Requested keycode was already the bound one; nothing was run.
    NoOp,
}

impl WebrootInstallOutcome {
    /// True when the agent changed on disk, so a reboot binds the new state.
    pub fn reboot_recommended(self) -> bool {
        matches!(self, Self::FreshInstall | Self::Upgraded)
    }
}

impl std::fmt::Display for WebrootInstallOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FreshInstall => write!(f, "fresh install"),
            Self::Upgraded => write!(f, "upgraded"),
            Self::ReKeyed => write!(f, "re-keyed in place"),
            Self::NoOp => write!(f, "no-op"),
        }
    }
}

/// Strips formatting so `SAEA-TAOG-EA3E-4DE9-868C` and `saeataogea3e4de9868c`
/// compare equal.
fn normalize_keycode(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Licence state from `HKLM\SOFTWARE\WOW6432Node\WRData` and its `Status` subkey.
#[derive(Debug, Clone, Default)]
struct WebrootLicence {
    is_expired: Option<u32>,
    days_remaining: Option<u32>,
    license_cat: Option<String>,
    /// Bound keycode, from `WRData\PULV`. The `Status` subkey never exposes it.
    keycode: Option<String>,
}

impl WebrootLicence {
    /// Licensed means not expired AND a non-empty category. An activated
    /// agent reports `IsExpired=0` with `license_cat=WSAV`; one that took an
    /// installer run but never bound a keycode leaves the category empty.
    fn is_licensed(&self) -> bool {
        self.is_expired == Some(0)
            && self.license_cat.as_deref().is_some_and(|c| !c.trim().is_empty())
    }

    /// True when the agent has `activation_key` bound right now.
    fn holds_keycode(&self, activation_key: &str) -> bool {
        let want = normalize_keycode(activation_key);
        !want.is_empty()
            && self
                .keycode
                .as_deref()
                .is_some_and(|k| normalize_keycode(k) == want)
    }

    fn summary(&self) -> String {
        format!(
            "IsExpired={} DaysRemaining={} license_cat={:?} keycode={}",
            self.is_expired.map_or_else(|| "?".into(), |v| v.to_string()),
            self.days_remaining.map_or_else(|| "?".into(), |v| v.to_string()),
            self.license_cat.as_deref().unwrap_or(""),
            if self.keycode.as_deref().is_some_and(|k| !k.is_empty()) { "set" } else { "unset" }
        )
    }
}

/// Reads Webroot's licence state. PowerShell rather than the `winreg` crate
/// to match `utilities::windows::antivirus`, which shells out for the same
/// reason.
async fn webroot_licence_state() -> WebrootLicence {
    let ps_cmd = r#"
$s = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\WRData\Status' -ErrorAction SilentlyContinue
$d = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\WRData' -ErrorAction SilentlyContinue
[PSCustomObject]@{
  IsExpired     = $s.IsExpired
  DaysRemaining = $s.DaysRemaining
  LicenseCat    = $s.license_cat
  Keycode       = ([string]$d.PULV).Trim([char]0)
} | ConvertTo-Json -Compress
"#;

    let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
    else {
        return WebrootLicence::default();
    };

    let stdout = String::from_utf8_lossy(&out.stdout);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout.trim()) else {
        return WebrootLicence::default();
    };

    // These land as either a JSON number or a decimal string depending on the
    // REG value type, so accept both.
    let as_u32 = |field: &str| -> Option<u32> {
        v.get(field).and_then(|x| {
            x.as_u64()
                .map(|n| n as u32)
                .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
        })
    };

    let as_string = |field: &str| -> Option<String> {
        v.get(field).and_then(|x| x.as_str()).map(str::to_string)
    };

    WebrootLicence {
        is_expired: as_u32("IsExpired"),
        days_remaining: as_u32("DaysRemaining"),
        license_cat: as_string("LicenseCat"),
        keycode: as_string("Keycode"),
    }
}

/// `WRSA.exe` file version, for telling a real upgrade from a same-version
/// no-op.
async fn wrsa_file_version() -> Option<String> {
    let ps_cmd = r#"
foreach ($p in 'C:\Program Files\Webroot\WRSA.exe','C:\Program Files (x86)\Webroot\WRSA.exe') {
  if (Test-Path $p) { (Get-Item $p).VersionInfo.FileVersion; break }
}
"#;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// Path of the installed `WRSA.exe`, if Webroot is present.
fn installed_wrsa_path() -> Option<PathBuf> {
    [
        r"C:\Program Files\Webroot\WRSA.exe",
        r"C:\Program Files (x86)\Webroot\WRSA.exe",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

/// The command that opens Webroot's own re-key flow with the keycode filled in.
///
/// `-kcswap=` is a real switch in WRSA.exe's command-line table, and it is the
/// same path as the GUI's "Activate a new keycode" — but Webroot gates the swap
/// behind a CAPTCHA, so it cannot complete unattended. Verified on WRSA
/// 9.0.45.63 (2026-08-01): the switch is parsed and opens the dialog, and the
/// agent's window tree exposes no UI Automation elements to drive it with.
fn webroot_rekey_command(exe: &PathBuf, activation_key: &str) -> String {
    format!("\"{}\" -kcswap={activation_key}", exe.display())
}

/// Runs the in-place swap and polls for the keycode to bind, giving up quickly.
///
/// Worth attempting on an agent holding no licence, where there is nothing to
/// lose and the swap may go through unprompted. When Webroot does raise its
/// CAPTCHA this just times out, leaving the prompt on screen for a technician.
async fn webroot_try_kcswap(exe: &PathBuf, activation_key: &str) -> WebrootLicence {
    const POLL_INTERVAL: Duration = Duration::from_secs(3);
    const POLL_TIMEOUT: Duration = Duration::from_secs(45);

    info!("Attempting in-place keycode swap: {} -kcswap=<key>", exe.display());
    // Not awaited: WRSA is a GUI process that outlives the swap.
    if let Err(e) = Command::new(exe)
        .arg(format!("-kcswap={activation_key}"))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        info!("Could not launch WRSA.exe for the swap: {e}");
        return webroot_licence_state().await;
    }

    let deadline = tokio::time::Instant::now() + POLL_TIMEOUT;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let licence = webroot_licence_state().await;
        if (licence.is_licensed() && licence.holds_keycode(activation_key))
            || tokio::time::Instant::now() >= deadline
        {
            return licence;
        }
    }
}

/// Installs and activates Webroot with `activation_key`.
///
/// Returns `Ok` only when the registry shows that keycode actually bound.
/// A live agent already on the CDN build cannot be re-keyed unattended — see
/// [`webroot_rekey_command`] — so that case returns `Err` naming the handoff
/// rather than reporting a success the agent never had.
pub async fn install_webroot(
    activation_key: String,
    client: Client,
    progress_tx: Sender<(u64, u64)>
) -> anyhow::Result<WebrootInstallOutcome, anyhow::Error> {
    let activation_key = activation_key.trim().to_string();
    if activation_key.is_empty() {
        return Err(anyhow::anyhow!("Activation key is empty"));
    }

    info!("running install_webroot!");

    let installed_exe = installed_wrsa_path();
    let already_installed = installed_exe.is_some();
    let version_before = if already_installed { wrsa_file_version().await } else { None };
    let licence_before = webroot_licence_state().await;
    info!(
        "install_webroot: already_installed={already_installed} version_before={version_before:?} {}",
        licence_before.summary()
    );

    // Nothing to do when the requested keycode is already the bound one. Saves
    // an 85 MB download, and avoids poking an agent that is already correct.
    if already_installed && licence_before.is_licensed() && licence_before.holds_keycode(&activation_key) {
        info!("install_webroot outcome: {}", WebrootInstallOutcome::NoOp);
        return Ok(WebrootInstallOutcome::NoOp);
    }

    let temp_directory = std::env::temp_dir();
    let wrv_path = format!("{}\\wsasme.exe", temp_directory.display());

    let need_download = match tokio::fs::metadata(&wrv_path).await {
        Ok(meta) if meta.len() > 500_000 => {
            info!("Cached Webroot installer found ({} bytes)", meta.len());
            false
        }
        _ => true,
    };

    if need_download {
        if let Err(e) = download_file(
            &client,
            "https://anywhere.webrootcloudav.com/zerol/wsasme.exe",
            &wrv_path,
            &progress_tx,
        ).await {
            info!("Webroot download failed ({e}), checking connectivity...");
            crate::utilities::windows::net_adapter::ensure_internet_connected().await?;
            download_file(
                &client,
                "https://anywhere.webrootcloudav.com/zerol/wsasme.exe",
                &wrv_path,
                &progress_tx,
            ).await?;
        }
    }

    #[cfg(target_os = "windows")]
    {
        info!("Running Webroot installer (waiting for completion)...");
        let output = Command::new("cmd")
            .arg("/C")
            .arg(&wrv_path)
            .arg(format!("/key={activation_key}"))
            .arg("/silent")
            .arg("-clone")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await?;

        info!("Webroot installer exit status: {:?}", output.status);

        if !output.status.success() {
            info!("Cached Webroot installer failed, re-downloading...");
            let _ = tokio::fs::remove_file(&wrv_path).await;
            download_file(
                &client,
                "https://anywhere.webrootcloudav.com/zerol/wsasme.exe",
                &wrv_path,
                &progress_tx,
            ).await?;

            let retry = Command::new("cmd")
                .arg("/C")
                .arg(&wrv_path)
                .arg(format!("/key={activation_key}"))
                .arg("/silent")
                .arg("-clone")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await?;
            info!("Webroot retry exit status: {:?}", retry.status);
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Verify rather than trust the exit code. `wsasme.exe` returns 0 when
        // it short-circuits against a live agent of the same version, so a
        // successful process says nothing about whether the keycode bound.
        let mut licence = webroot_licence_state().await;
        let version_after = wrsa_file_version().await;
        let version_changed = already_installed && version_before != version_after;
        info!(
            "install_webroot verify: version_after={version_after:?} version_changed={version_changed} {}",
            licence.summary()
        );

        let mut bound = licence.is_licensed() && licence.holds_keycode(&activation_key);

        // The installer only binds a keycode while replacing binaries, so it is
        // a silent no-op against an agent already on the CDN build. Try the
        // agent's own in-place swap before giving up.
        if !bound {
            if let Some(exe) = installed_exe.as_ref() {
                licence = webroot_try_kcswap(exe, &activation_key).await;
                bound = licence.is_licensed() && licence.holds_keycode(&activation_key);
                info!("install_webroot: after -kcswap {}", licence.summary());
            }
        }

        if !bound {
            let handoff = installed_exe.as_ref().map_or_else(
                || " Webroot is not installed and the installer did not put it there.".to_string(),
                |exe| {
                    format!(
                        " The agent is still {}, so the installer had nothing to upgrade, and the \
                         in-place swap did not complete on its own — Webroot can gate it behind a \
                         CAPTCHA, and its prompt may now be waiting on the machine's screen. \
                         Finish it there, in the UI (WRSA > My Account > 'Activate a new keycode') \
                         or by running {}.",
                        version_after.as_deref().unwrap_or("an unknown version"),
                        webroot_rekey_command(exe, &activation_key)
                    )
                },
            );
            return Err(anyhow::anyhow!(
                "Webroot is NOT activated with this keycode ({}).{}",
                licence.summary(),
                handoff
            ));
        }

        let outcome = if !already_installed {
            WebrootInstallOutcome::FreshInstall
        } else if version_changed {
            WebrootInstallOutcome::Upgraded
        } else {
            WebrootInstallOutcome::ReKeyed
        };
        info!("install_webroot outcome: {outcome}");
        return Ok(outcome);
    }

    #[cfg(not(target_os = "windows"))]
    Ok(if already_installed {
        WebrootInstallOutcome::NoOp
    } else {
        WebrootInstallOutcome::FreshInstall
    })
}

/// Applies `activation_key` to an installed SAS and returns the window title
/// that proves the Professional subscription took.
///
/// `SUPERAntiSpyware.exe /autoregister:` writes the registration within seconds
/// and then stays resident as the tray application, so awaiting process exit
/// never returns. This spawns it, polls for the subscription window, then kills
/// the wrapper's process tree.
async fn run_sas_autoregister(
    sas_exe: &std::path::Path,
    activation_key: &str,
) -> anyhow::Result<String> {
    const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(90);
    const POLL_INTERVAL: Duration = Duration::from_secs(2);

    // A window left over from an earlier activation would read as this run's proof.
    if let Some(stale) = sas_pro_window_title_async().await {
        info!("Pre-existing SAS Professional window ({stale}); killing SAS first");
        kill_sas_processes();
        tokio::time::sleep(Duration::from_secs(3)).await;
        if let Some(stale) = sas_pro_window_title_async().await {
            return Err(anyhow::anyhow!(
                "A SUPERAntiSpyware Professional window ({stale}) survived a process kill, so \
                 this activation cannot be verified. Reboot and re-run."
            ));
        }
    }

    info!("SAS EXE: cmd /c {sas_exe:?} /autoregister:{activation_key}");
    let mut child = Command::new("cmd")
        .arg("/C")
        .arg(sas_exe.as_os_str())
        .arg(format!("/autoregister:{activation_key}"))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;

    let deadline = tokio::time::Instant::now() + ACTIVATION_TIMEOUT;
    let mut title = None;
    let mut early_exit = None;
    while title.is_none() && early_exit.is_none() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(POLL_INTERVAL).await;
        title = sas_pro_window_title_async().await;
        if title.is_none() {
            // cmd waits on the child, so an exited wrapper means SAS did not
            // stay resident and no window is coming.
            early_exit = child.try_wait().ok().flatten();
        }
    }

    if let Some(pid) = child.id() {
        taskkill_tree(pid);
    }
    let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;

    if let Some(title) = title {
        info!("SAS activated: {title}");
        return Ok(title);
    }
    Err(match early_exit {
        Some(status) => anyhow::anyhow!(
            "SUPERAntiSpyware.exe /autoregister exited ({status:?}) without showing a \
             Professional subscription window — the key did not bind."
        ),
        None => anyhow::anyhow!(
            "SUPERAntiSpyware showed no Professional subscription window within {}s of \
             /autoregister — the key did not bind.",
            ACTIVATION_TIMEOUT.as_secs()
        ),
    })
}

pub async fn install_sas(
    activation_key: String,
    client: Client,
    progress_tx: Sender<(u64, u64)>,
) -> anyhow::Result<String, anyhow::Error> {
    if activation_key.is_empty() {
        return Err(anyhow::anyhow!("Activation key is empty"));
    }

    // If SAS is already installed, just kill any running processes and run
    // the in-place /autoregister against the existing executable. This is the
    // proven path that has worked for years — direct DB writes corrupted the
    // SQLite store and broke the uninstaller.
    if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
        for program in &mut *programs {
            if let (Some(publisher), Some(install_location)) = (&program.publisher, &program.install_location) {
                if program.display_name.clone().unwrap_or_default().contains("SUPERAntiSpyware")
                    || publisher.clone().contains("SUPERAntiSpyware")
                {
                    let sas_exe = PathBuf::from(install_location).join("SUPERAntiSpyware.exe");
                    if sas_exe.exists() {
                        info!("SAS already installed, killing processes before autoregister");
                        kill_sas_processes();
                        tokio::time::sleep(Duration::from_secs(3)).await;

                        return run_sas_autoregister(&sas_exe, &activation_key).await;
                    } else {
                        info!("Install location not found: {sas_exe:?}");
                    }
                }
            }
        }
    }

    info!("SAS not found, downloading and installing...");

    let temp_directory = std::env::temp_dir();
    let sas_path = format!("{}\\sas.exe", temp_directory.display());

    let need_download = match tokio::fs::metadata(&sas_path).await {
        Ok(meta) if meta.len() > 1_000_000 => {
            info!("Cached SAS installer found ({} bytes), trying it first", meta.len());
            false
        }
        _ => true,
    };

    if need_download {
        if let Err(e) = download_file(
            &client,
            "https://secure.superantispyware.com/SUPERAntiSpyware.exe",
            &sas_path,
            &progress_tx,
        ).await {
            info!("SAS download failed ({e}), checking connectivity...");
            crate::utilities::windows::net_adapter::ensure_internet_connected().await?;
            download_file(
                &client,
                "https://secure.superantispyware.com/SUPERAntiSpyware.exe",
                &sas_path,
                &progress_tx,
            ).await?;
        }
    }

    #[cfg(target_os = "windows")]
    {
        info!("Running SAS installer (waiting for completion)...");
        let installer_output = Command::new("cmd")
            .arg("/C")
            .arg(&sas_path)
            .arg(format!("/REGCODE={activation_key}"))
            .arg("/silent")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await?;

        info!("SAS installer exit status: {:?}", installer_output.status);

        if !installer_output.status.success() {
            info!("Cached installer failed, re-downloading...");
            let _ = tokio::fs::remove_file(&sas_path).await;
            download_file(
                &client,
                "https://secure.superantispyware.com/SUPERAntiSpyware.exe",
                &sas_path,
                &progress_tx,
            ).await?;

            let retry = Command::new("cmd")
                .arg("/C")
                .arg(&sas_path)
                .arg(format!("/REGCODE={activation_key}"))
                .arg("/silent")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await?;
            info!("SAS installer retry exit status: {:?}", retry.status);
        }

        tokio::time::sleep(Duration::from_secs(10)).await;

        let killed = kill_sas_processes();
        info!("Killed {killed} SAS processes post-install");
        tokio::time::sleep(Duration::from_secs(3)).await;

        let sas_exe = PathBuf::from(r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe");
        if !sas_exe.exists() {
            return Err(anyhow::anyhow!(
                "The SUPERAntiSpyware installer finished but {} does not exist, so nothing was \
                 activated.",
                sas_exe.display()
            ));
        }
        info!("Running autoregister after fresh install");
        return run_sas_autoregister(&sas_exe, &activation_key).await;
    }

    #[cfg(not(target_os = "windows"))]
    Err(anyhow::anyhow!("SUPERAntiSpyware is Windows-only"))
}


pub async fn install_supereasybackup(
    customer_email: String, 
    client: Client,
    progress_tx: Sender<(u64, u64)>,
) -> anyhow::Result<(), anyhow::Error> {
    info!("running install_supereasybackup!");

    let temp_directory = std::env::temp_dir();
    let seb_path = format!("{}\\seb.msi", temp_directory.display());

    let need_download = match tokio::fs::metadata(&seb_path).await {
        Ok(meta) if meta.len() > 500_000 => {
            info!("Cached SEB installer found ({} bytes)", meta.len());
            false
        }
        _ => true,
    };

    if need_download {
        if let Err(e) = download_file(
            &client,
            "https://dcgeneral.blob.core.windows.net/downloads/MUS/v11.5.0/DCProtect-11.5.0.8737-SuperEasyBackup.msi",
            &seb_path,
            &progress_tx,
        ).await {
            info!("SEB download failed ({e}), checking connectivity...");
            crate::utilities::windows::net_adapter::ensure_internet_connected().await?;
            download_file(
                &client,
                "https://dcgeneral.blob.core.windows.net/downloads/MUS/v11.5.0/DCProtect-11.5.0.8737-SuperEasyBackup.msi",
                &seb_path,
                &progress_tx,
            ).await?;
        }
    }

    let response_json: Vec<CarboniteResponse> = CarboniteResponse::default()
        .from_customer_email(customer_email, client.clone()).await?;

    if response_json.is_empty() { return Err(anyhow::anyhow!("Response is empty")); }

    if let Some(carbonite_entry) = find_latest_carbonite_entry(&response_json) {
        let activation_code = &carbonite_entry.activation_code;
        #[cfg(target_os = "windows")]
        {
            let cmd_string = format!(
                "msiexec /i \"{}\" /qn Silent=1 ActivationURL=https://blue.mysecuredatavault.com ActivationCode={}",
                seb_path, activation_code
            );

            info!("Running SEB installer: {:?}", cmd_string);

            let output = Command::new("powershell")
                .arg("-Command")
                .arg(cmd_string)
                .creation_flags(0x08000000)
                .output()
                .await?;

            info!("SEB installer exit status: {:?}", output.status);

            // Give installer a moment to finish placing files, then launch Super Easy Backup endpoint UI
            const SEB_DCPROTECT_PATH: &str = r"C:\Program Files (x86)\Super Easy Backup\endpoint\dcprotect.exe";
            for _ in 0..5 {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                if std::path::Path::new(SEB_DCPROTECT_PATH).exists() {
                    break;
                }
            }
            if std::path::Path::new(SEB_DCPROTECT_PATH).exists() {
                if let Err(e) = std::process::Command::new(SEB_DCPROTECT_PATH).spawn() {
                    info!("Failed to launch SEB dcprotect.exe: {e}");
                } else {
                    info!("Launched Super Easy Backup: dcprotect.exe");
                }
            }
        }
    }
    Ok(())
}

/// Downloads a file from `url` into `dest_path`, streaming bytes and reporting progress.
pub async fn download_file(
    client: &Client,
    url: &str,
    dest_path: &str,
    progress_tx: &Sender<(u64, u64)>,
) -> anyhow::Result<()> {
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
    info!("Download complete ({dest_path}). SHA-256: {:x}", hash);
    Ok(())
}
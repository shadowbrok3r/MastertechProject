//! Non-destructive OS provisioning steps (ported from QCWizard `Procedure` /
//! `OperatingSystem`): core isolation, timezone, and opening the system tools.
//! Windows-only; stubs error elsewhere. Run under the app's admin manifest.

#[cfg(windows)]
mod imp {
    use anyhow::{anyhow, Context};
    use std::process::Command;

    const CORE_ISOLATION_KEY: &str =
        r"HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity";

    /// Enable HVCI (core isolation): `Enabled = 1`. Requires elevation.
    pub fn enable_core_isolation() -> anyhow::Result<String> {
        let out = Command::new("reg")
            .args(["add", CORE_ISOLATION_KEY, "/v", "Enabled", "/t", "REG_DWORD", "/d", "1", "/f"])
            .output()
            .context("spawn reg.exe")?;
        if out.status.success() {
            Ok("Core isolation (HVCI) enabled — reboot required to take effect.".into())
        } else {
            Err(anyhow!("reg add failed: {}", String::from_utf8_lossy(&out.stderr)))
        }
    }

    pub fn set_timezone_mountain() -> anyhow::Result<String> {
        let out = Command::new("tzutil")
            .args(["/s", "Mountain Standard Time"])
            .output()
            .context("spawn tzutil")?;
        if out.status.success() {
            Ok("Timezone set to Mountain Standard Time.".into())
        } else {
            Err(anyhow!("tzutil failed: {}", String::from_utf8_lossy(&out.stderr)))
        }
    }

    /// Open Disk Management, About, and Device Manager for the tech to eyeball.
    pub fn open_system_tools() -> anyhow::Result<String> {
        let _ = Command::new("mmc").arg("diskmgmt.msc").spawn();
        let _ = Command::new("explorer").arg("ms-settings:about").spawn();
        let _ = Command::new("mmc").arg("devmgmt.msc").spawn();
        Ok("Opened Disk Management, About, and Device Manager.".into())
    }

    pub fn open_wifi_settings() -> anyhow::Result<String> {
        Command::new("explorer").arg("ms-settings:network-wifi").spawn().context("spawn explorer")?;
        Ok("Opened Wi-Fi settings.".into())
    }

    pub fn open_share_browser() -> anyhow::Result<String> {
        Command::new("explorer").arg(r"\\winbits7\copyfolder").spawn().context("spawn explorer")?;
        Ok("Opened install share.".into())
    }

    pub fn open_windows_update() -> anyhow::Result<String> {
        Command::new("explorer").arg("ms-settings:windowsupdate").spawn().context("spawn explorer")?;
        Ok("Opened Windows Update.".into())
    }

    /// Restart Explorer to refresh the Start menu.
    pub fn fix_start_menu() -> anyhow::Result<String> {
        let _ = Command::new("taskkill").args(["/f", "/im", "explorer.exe"]).output();
        Command::new("explorer.exe").spawn().context("spawn explorer")?;
        Ok("Restarted Explorer (Start menu refresh).".into())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn enable_core_isolation() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("core isolation is Windows-only"))
    }
    pub fn set_timezone_mountain() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("timezone set is Windows-only"))
    }
    pub fn open_system_tools() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("system tools are Windows-only"))
    }
    pub fn open_wifi_settings() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("Wi-Fi settings are Windows-only"))
    }
    pub fn open_share_browser() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("share browser is Windows-only"))
    }
    pub fn open_windows_update() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("Windows Update is Windows-only"))
    }
    pub fn fix_start_menu() -> anyhow::Result<String> {
        Err(anyhow::anyhow!("Start menu fix is Windows-only"))
    }
}

pub use imp::{
    enable_core_isolation, fix_start_menu, open_share_browser, open_system_tools,
    open_wifi_settings, open_windows_update, set_timezone_mountain,
};

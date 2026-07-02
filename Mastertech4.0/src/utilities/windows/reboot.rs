//! Reboot with MasterTech auto-relaunch via a one-shot logon scheduled task.

pub const RELAUNCH_TASK_NAME: &str = "MastertechAutoRestart";

/// Escapes a value for a single-quoted PowerShell string literal.
#[cfg(target_os = "windows")]
fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Registers a logon task that relaunches MasterTech with its working directory
/// set to the exe directory, so relative files like data.enc resolve.
#[cfg(target_os = "windows")]
pub async fn schedule_mastertech_relaunch(terminal_mode: bool) -> anyhow::Result<()> {
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("exe path has no parent directory"))?
        .to_path_buf();

    let exe = ps_quote(&exe_path.to_string_lossy());
    let dir = ps_quote(&exe_dir.to_string_lossy());
    let action = if terminal_mode {
        format!("New-ScheduledTaskAction -Execute '{exe}' -Argument '-t' -WorkingDirectory '{dir}'")
    } else {
        format!("New-ScheduledTaskAction -Execute '{exe}' -WorkingDirectory '{dir}'")
    };
    let script = format!(
        "$a={action};$t=New-ScheduledTaskTrigger -AtLogOn;Register-ScheduledTask -TaskName '{RELAUNCH_TASK_NAME}' -Action $a -Trigger $t -RunLevel Highest -Force | Out-Null"
    );

    let out = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000)
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "Register-ScheduledTask failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    log::info!("Registered {RELAUNCH_TASK_NAME} logon task ({})", exe_path.display());
    Ok(())
}

/// Deletes the relaunch task so it fires once per reboot, not every logon.
#[cfg(target_os = "windows")]
pub fn clear_relaunch_task() {
    std::thread::spawn(|| {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("schtasks")
            .args(["/delete", "/tn", RELAUNCH_TASK_NAME, "/f"])
            .creation_flags(0x08000000)
            .output();
    });
}

/// Initiates a system reboot after a short delay.
#[cfg(target_os = "windows")]
pub async fn reboot_now(comment: &str) -> anyhow::Result<()> {
    let out = tokio::process::Command::new("shutdown")
        .args(["/r", "/t", "5", "/c", comment])
        .creation_flags(0x08000000)
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "shutdown failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Schedules the relaunch task then reboots; errors are logged, not fatal.
#[cfg(target_os = "windows")]
pub fn spawn_reboot_with_relaunch(terminal_mode: bool, comment: &'static str) {
    tokio::spawn(async move {
        if let Err(e) = schedule_mastertech_relaunch(terminal_mode).await {
            log::error!("schedule_mastertech_relaunch: {e}");
        }
        if let Err(e) = reboot_now(comment).await {
            log::error!("reboot_now: {e}");
        }
    });
}

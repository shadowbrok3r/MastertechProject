use rusqlite::Connection;
use uuid::Uuid;
use std::path::PathBuf;
use std::process::Command;

const SAS_EXE: &str = r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe";

/// SAS_CURRENTUSER.DB3 SETTINGS table — complete reference configuration.
///
/// Type codes: 256 = null-terminated UTF-8 text, 257 = null-terminated UTF-16LE wide text,
/// 259 = 4-byte little-endian integer, 263 = raw binary blob.
///
/// Changes vs. original captured DB:
///   • Added  NotifyHomePageChanged  = "no"   (was missing — caused notify to remain enabled)
///   • Added  ProtectedHomePage      = ""      (was missing — SAS reads this alongside ProtectHomePage)
///   • Added  ValidationRand         = 0       (was missing — present in actual DB schema)
///   • Changed DoNotShowSOSToaster   = "yes"   (was "no" — suppress the SOS renewal toaster)
const REFERENCE_SETTINGS: &[(i32, &str, i32, &str)] = &[
    (1,  "PreConfigurationComplete",                  256, "79657300"),          // "yes"
    (2,  "ShowSplashScreen",                          256, "6e6f00"),            // "no"
    (3,  "NotifyHomePageChanged",                     256, "6e6f00"),            // "no"  ← ADDED
    (4,  "NotifySpywareBlocked",                      256, "6e6f00"),            // "no"
    (5,  "NotifyStartupItemChanged",                  256, "6e6f00"),            // "no"
    (6,  "EnableRealTimeProtection",                  256, "79657300"),          // "yes"
    (7,  "ScanUpdateCheck",                           256, "6e6f00"),            // "no"
    (8,  "CheckForUpdates",                           256, "79657300"),          // "yes"
    (9,  "CheckForUpdatesOnStartup",                  256, "79657300"),          // "yes"
    (10, "CheckForUpdatesInterval",                   259, "0c000000"),          // 12
    (11, "ColorSet",                                  256, "5341532044656661756c7400"), // "SAS Default"
    (12, "NotifyAdBlockSoundPath",                    256, "00"),                // ""
    (13, "NotifyAdBlockSoundPathW",                   257, "43003a005c00500072006f006700720061006d002000460069006c00650073005c005300550050004500520041006e007400690053007000790077006100720065005c006400650074006500630074002e007700610076000000"),
    (14, "NotifyPlaySound",                           256, "6e6f00"),            // "no"
    (15, "EventLoggingActive",                        256, "6e6f00"),            // "no"
    (16, "EventLoggingFlags",                         259, "00000000"),          // 0
    (17, "OptionalDisplayItems",                      263, "0101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101"),
    (18, "ProtectHomePage",                           256, "6e6f00"),            // "no"
    (19, "ProtectedHomePage",                         256, "00"),                // ""  ← ADDED
    (20, "VersionProcessList",                        259, "504a0000"),
    (21, "VersionProcessListRelated",                 259, "c4410000"),
    (22, "UNCUpdateServerPath",                       256, "00"),                // ""
    (23, "SilentUpdates",                             256, "79657300"),          // "yes"
    (24, "TerminationProtection",                     256, "6e6f00"),            // "no"
    (25, "TerminationProtectionAllowedTrusted",       256, "6e6f00"),            // "no"
    (26, "IntegrateWithSecurityCenter",               256, "79657300"),          // "yes"
    (27, "UpgradeToProfessionalCompleted",            256, "79657300"),          // "yes"
    (28, "ScanAutoCleanLogs",                         256, "6e6f00"),            // "no"
    (29, "ScanAutoCleanLogsDays",                     259, "1e000000"),          // 30
    (30, "ScanAutoCleanQuarantine",                   256, "79657300"),          // "yes"
    (31, "ScanAutoCleanQuarantineDays",               259, "1e000000"),          // 30
    (32, "ScanScheduleEnabled",                       256, "79657300"),          // "yes"
    (33, "ScanSkipLargeFiles",                        256, "79657300"),          // "yes"
    (34, "ScanCleanCookies",                          256, "79657300"),          // "yes"
    (35, "ScanLastScanTime",                          263, "00000000000000000000000000000000"),
    (36, "ScanLastDefinitionRemindTime",              263, "ea070300010009000f00120010006f00"),
    (37, "ScanRemindCheckForDefinitionUpdates",       256, "6e6f00"),            // "no"
    (38, "ScanShowBalloonUpdateStatus ",              256, "6e6f00"),            // "no"  (trailing space is intentional — matches SAS key name)
    (39, "ScanRemindCheckForDefinitionUpdatesDays",   259, "05000000"),          // 5
    (40, "ScanMinFileSize",                           259, "00004000"),
    (41, "ScanOnlyKnownFileTypes",                    256, "79657300"),          // "yes"
    (42, "ScanIgnoreNonExecutableFiles",              256, "79657300"),          // "yes"
    (43, "ScanIgnoreSystemRestore",                   256, "6e6f00"),            // "no"
    (44, "ScanShowIconInSystemTray",                  256, "79657300"),          // "yes"
    (45, "ScanKeepLogs",                              256, "79657300"),          // "yes"
    (46, "ScanKeepCleanLogs",                         256, "79657300"),          // "yes"
    (47, "ScanCustomMemory",                          256, "79657300"),          // "yes"
    (48, "ScanCustomRegistry",                        256, "79657300"),          // "yes"
    (49, "ScanCustomStartup",                         256, "79657300"),          // "yes"
    (50, "ScanCustomFolders",                         256, "79657300"),          // "yes"
    (51, "ScanCustomCookies",                         256, "79657300"),          // "yes"
    (52, "ScanAutoScanType",                          259, "03000000"),          // 3
    (53, "ScanAutoScanCheckForUpdates",               256, "79657300"),          // "yes"
    (54, "ScanAutoScanHideUserInterface",             256, "6e6f00"),            // "no"
    (55, "ScanScheduleCheckForUpdates",               256, "6e6f00"),            // "no"
    (56, "ScanCloseBrowsers",                         256, "6e6f00"),            // "no"
    (57, "ScanClearTemp",                             256, "6e6f00"),            // "no"
    (58, "ScanResolveLinks",                          256, "79657300"),          // "yes"
    (59, "ScanTerminateMemoryThreats",                256, "6e6f00"),            // "no"
    (60, "ScanUseKernelFileDirect",                   256, "79657300"),          // "yes"
    (61, "ScanUseKernelRegistryDirect",               256, "79657300"),          // "yes"
    (62, "ScanUseDirectDiskAccess",                   256, "79657300"),          // "yes"
    (63, "ScanADS",                                   256, "79657300"),          // "yes"
    (64, "ScanDisplayContextMenu",                    256, "79657300"),          // "yes"
    (65, "ScanBoostActive",                           256, "79657300"),          // "yes"
    (66, "ScanBoostLevel",                            259, "feffffff"),          // -2 (max boost)
    (67, "ScanUnwanted",                              256, "79657300"),          // "yes"
    (68, "ScanModifiedFilesOnly",                     256, "6e6f00"),            // "no"
    (69, "ScanModifiedFilesDays",                     259, "1e000000"),          // 30
    (70, "ScanArchiveFlags",                          259, "00000000"),          // 0
    (71, "DoNotShowSOSToaster",                       256, "79657300"),          // "yes" ← CHANGED (was "no")
    (72, "ValidationRand",                            259, "00000000"),          // 0  ← ADDED
    // GameModeDuration: ffffffff = -1 = "indefinite" duration when DND is triggered.
    // Note: the DND *enabled* state is a runtime flag, not stored in this table.
    (73, "GameModeDuration",                          259, "ffffffff"),          // -1 (indefinite)
];

// ─── helpers ────────────────────────────────────────────────────────────────

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

// ─── DB paths ────────────────────────────────────────────────────────────────

/// SAS_CURRENTUSER.DB3 — per-user settings stored under %APPDATA%.
pub fn get_sas_currentuser_db_path() -> anyhow::Result<PathBuf> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| anyhow::anyhow!("APPDATA not set"))?;
    let p = PathBuf::from(appdata)
        .join("SUPERAntiSpyware.com")
        .join("SUPERAntiSpyware")
        .join("SAS_CURRENTUSER.DB3");
    if !p.exists() {
        return Err(anyhow::anyhow!("SAS_CURRENTUSER.DB3 not found at {}", p.display()));
    }
    Ok(p)
}

// NOTE: We deliberately do NOT touch SAS_ALLUSER.DB3 here. Direct writes to
// the shared activation database corrupted the SQLite store, broke the SAS
// uninstaller, and caused the UI to misreport licence state. Activation is
// performed only through the official `/REGCODE` installer flag and the
// `/autoregister:KEY` CLI on the installed executable. See `install_sas` in
// `Mastertech4.0/src/utilities/scripts/antivirus/mod.rs`.

// ─── CURRENTUSER settings ────────────────────────────────────────────────────

/// Wipe and repopulate the SETTINGS table in SAS_CURRENTUSER.DB3.
fn apply_sas_settings(conn: &Connection) -> anyhow::Result<usize> {
    conn.execute("DELETE FROM SETTINGS", [])?;
    let mut count = 0;
    for &(id, name, type_code, data_hex) in REFERENCE_SETTINGS {
        let data = hex_to_bytes(data_hex);
        conn.execute(
            "INSERT INTO SETTINGS (id, name, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, name, type_code, data],
        )?;
        count += 1;
    }
    log::info!("Applied {count} SAS settings to CURRENTUSER DB");
    Ok(count)
}

// ─── Start-with-Windows ───────────────────────────────────────────────────────

/// Add SAS to HKCU\...\Run so it launches automatically at Windows login.
#[cfg(target_os = "windows")]
pub fn add_sas_startup_run_key() -> anyhow::Result<()> {
    use windows_registry::CURRENT_USER;
    let key = CURRENT_USER
        .options()
        .read()
        .write()
        .create()
        .open(r"Software\Microsoft\Windows\CurrentVersion\Run")?;
    key.set_string("SUPERAntiSpyware", SAS_EXE)?;
    log::info!("Added SAS startup Run key");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn add_sas_startup_run_key() -> anyhow::Result<()> {
    Ok(())
}

// ─── Task XML templates ───────────────────────────────────────────────────────

const UPDATE_TASK_XML: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>SUPERAntiSpyware</Author>
    <Description>SUPERAntiSpyware Scheduled Task</Description>
    <URI>\SUPERAntiSpyware\SUPERAntiSpyware Scheduled Task {TASK_GUID}</URI>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <Repetition>
        <Interval>PT8H</Interval>
        <Duration>PT23H59M</Duration>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>{START_TIME}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{USER_SID}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>true</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>true</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <Duration>PT10M</Duration>
      <WaitTimeout>PT1H</WaitTimeout>
      <StopOnIdleEnd>true</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT72H</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>C:\Program Files\SUPERAntiSpyware\SASTask.exe</Command>
      <Arguments>"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe" /TASK:{TASK_GUID}</Arguments>
    </Exec>
  </Actions>
</Task>"#;

const QUICK_SCAN_TASK_XML: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>SUPERAntiSpyware</Author>
    <Description>SUPERAntiSpyware Scheduled Task</Description>
    <URI>\SUPERAntiSpyware\SUPERAntiSpyware Scheduled Task {TASK_GUID}</URI>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{START_TIME}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByWeek>
        <DaysOfWeek>
          <Monday />
          <Thursday />
        </DaysOfWeek>
        <WeeksInterval>1</WeeksInterval>
      </ScheduleByWeek>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{USER_SID}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>true</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>true</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <Duration>PT10M</Duration>
      <WaitTimeout>PT1H</WaitTimeout>
      <StopOnIdleEnd>true</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>true</WakeToRun>
    <ExecutionTimeLimit>PT72H</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>C:\Program Files\SUPERAntiSpyware\SASTask.exe</Command>
      <Arguments>"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe" /TASK:{TASK_GUID}</Arguments>
    </Exec>
  </Actions>
</Task>"#;

struct SasTask {
    guid: String,
    task_type: &'static str,
    time: &'static str,
    days: &'static str,
    hours: i32,
    wake: &'static str,
    clean: &'static str,
    hide: &'static str,
    updatebefore: &'static str,
    runifmissed: &'static str,
    xml_template: &'static str,
}

fn get_current_user_sid() -> anyhow::Result<String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "([System.Security.Principal.WindowsIdentity]::GetCurrent()).User.Value",
        ])
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to get current user SID"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn now_iso8601() -> String {
    Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Date -Format 'yyyy-MM-ddTHH:mm:ss'"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "2026-01-01T14:00:00".to_string())
}

// ─── Core settings + tasks writer ────────────────────────────────────────────

/// Write SETTINGS + ScheduledTasks to SAS_CURRENTUSER.DB3 and add the
/// startup Run registry key. Returns (update_task_guid, scan_task_guid).
fn configure_sas_settings_and_tasks() -> anyhow::Result<(String, String)> {
    let db_path = get_sas_currentuser_db_path()?;
    let user_sid = get_current_user_sid()?;
    let start_time = now_iso8601();

    log::info!("SAS CURRENTUSER DB: {}", db_path.display());

    let tasks = [
        SasTask {
            guid: Uuid::new_v4().to_string(),
            task_type: "UPDATE",
            time: "15:00",
            days: "",
            hours: 8,
            wake: "no",
            clean: "no",
            hide: "yes",
            updatebefore: "no",
            runifmissed: "yes",
            xml_template: UPDATE_TASK_XML,
        },
        SasTask {
            guid: Uuid::new_v4().to_string(),
            task_type: "QUICK_SCAN",
            time: "14:00",
            days: "MO TH ",
            hours: 0,
            wake: "yes",
            clean: "yes",
            hide: "yes",
            updatebefore: "yes",
            runifmissed: "no",
            xml_template: QUICK_SCAN_TASK_XML,
        },
    ];

    let conn = Connection::open(&db_path)?;
    let settings_count = apply_sas_settings(&conn)?;
    log::info!("Wrote {settings_count} settings to SAS CURRENTUSER DB");

    conn.execute("DELETE FROM ScheduledTasks", [])?;
    for (i, task) in tasks.iter().enumerate() {
        conn.execute(
            "INSERT INTO ScheduledTasks (id, taskid, type, time, days, hours, wake, restart, shutdown, clean, hide, updatebefore, runifmissed, lastruntime, disabled) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                (i + 1) as i32,
                &task.guid,
                task.task_type,
                task.time,
                task.days,
                task.hours,
                task.wake,
                "no",
                "no",
                task.clean,
                task.hide,
                task.updatebefore,
                task.runifmissed,
                &start_time,
                "no",
            ],
        )?;
        log::info!("Inserted SAS task: type={} guid={}", task.task_type, task.guid);
    }
    drop(conn);

    // Register tasks with Windows Task Scheduler
    let temp_dir = std::env::temp_dir();
    for task in &tasks {
        let xml = task
            .xml_template
            .replace("{TASK_GUID}", &task.guid)
            .replace("{USER_SID}", &user_sid)
            .replace("{START_TIME}", &start_time);

        let xml_path = temp_dir.join(format!("sas_task_{}.xml", task.guid));
        let mut utf16: Vec<u8> = vec![0xFF, 0xFE];
        for code_unit in xml.encode_utf16() {
            utf16.extend_from_slice(&code_unit.to_le_bytes());
        }
        std::fs::write(&xml_path, &utf16)?;

        let task_name = format!(
            "\\SUPERAntiSpyware\\SUPERAntiSpyware Scheduled Task {}",
            task.guid
        );

        let _ = Command::new("schtasks")
            .args(["/Create", "/TN", "\\SUPERAntiSpyware\\placeholder", "/SC", "ONCE", "/ST", "00:00", "/TR", "cmd /c echo noop", "/F"])
            .output();
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", "\\SUPERAntiSpyware\\placeholder", "/F"])
            .output();

        let out = Command::new("schtasks")
            .args(["/Create", "/XML", &xml_path.to_string_lossy(), "/TN", &task_name, "/F"])
            .output()?;
        if out.status.success() {
            log::info!("Registered task: {task_name}");
        } else {
            log::error!("Failed to register {task_name}: {}", String::from_utf8_lossy(&out.stderr));
        }
        let _ = std::fs::remove_file(&xml_path);
    }

    // Ensure SAS launches at Windows login
    if let Err(e) = add_sas_startup_run_key() {
        log::warn!("Could not add SAS startup Run key: {e}");
    }

    Ok((tasks[0].guid.clone(), tasks[1].guid.clone()))
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Apply the SAS settings reference table to SAS_CURRENTUSER.DB3 and register
/// the two SAS scheduled tasks (Update + Quick Scan) with Windows. Returns the
/// `(update_task_guid, quick_scan_task_guid)` pair for logging.
///
/// **Activation is intentionally not handled here** — use SAS's own
/// `/REGCODE` (installer) or `/autoregister:KEY` (existing exe) flow instead.
/// Direct writes to SAS_ALLUSER.DB3 corrupted the SQLite store and broke the
/// uninstaller, so we keep DB writes scoped to the per-user settings file.
pub fn configure_sas_scheduled_tasks() -> anyhow::Result<(String, String)> {
    configure_sas_settings_and_tasks()
}

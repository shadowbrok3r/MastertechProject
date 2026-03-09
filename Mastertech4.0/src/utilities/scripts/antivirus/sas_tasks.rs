use rusqlite::Connection;
use uuid::Uuid;
use std::path::PathBuf;
use std::process::Command;

/// Reference SETTINGS table from a known-good SAS configuration.
/// Each entry: (id, name, type_code, data_hex)
/// Captured from the master DB so every install gets identical behavior.
const REFERENCE_SETTINGS: &[(i32, &str, i32, &str)] = &[
    (1,  "PreConfigurationComplete",                  256, "79657300"),
    (2,  "ShowSplashScreen",                          256, "6e6f00"),
    (3,  "NotifySpywareBlocked",                      256, "6e6f00"),
    (4,  "NotifyStartupItemChanged",                  256, "6e6f00"),
    (5,  "EnableRealTimeProtection",                  256, "79657300"),
    (6,  "ScanUpdateCheck",                           256, "6e6f00"),
    (7,  "CheckForUpdates",                           256, "79657300"),
    (8,  "CheckForUpdatesOnStartup",                  256, "79657300"),
    (9,  "CheckForUpdatesInterval",                   259, "0c000000"),
    (10, "ColorSet",                                  256, "5341532044656661756c7400"),
    (11, "NotifyAdBlockSoundPath",                    256, "00"),
    (12, "NotifyAdBlockSoundPathW",                   257, "43003a005c00500072006f006700720061006d002000460069006c00650073005c005300550050004500520041006e007400690053007000790077006100720065005c006400650074006500630074002e007700610076000000"),
    (13, "NotifyPlaySound",                           256, "6e6f00"),
    (14, "EventLoggingActive",                        256, "6e6f00"),
    (15, "EventLoggingFlags",                         259, "00000000"),
    (16, "OptionalDisplayItems",                      263, "0101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101"),
    (17, "ProtectHomePage",                           256, "6e6f00"),
    (18, "VersionProcessList",                        259, "504a0000"),
    (19, "VersionProcessListRelated",                 259, "c4410000"),
    (20, "UNCUpdateServerPath",                       256, "00"),
    (21, "SilentUpdates",                             256, "79657300"),
    (22, "TerminationProtection",                     256, "6e6f00"),
    (23, "TerminationProtectionAllowedTrusted",       256, "6e6f00"),
    (24, "IntegrateWithSecurityCenter",               256, "79657300"),
    (25, "UpgradeToProfessionalCompleted",            256, "79657300"),
    (26, "ScanAutoCleanLogs",                         256, "6e6f00"),
    (27, "ScanAutoCleanLogsDays",                     259, "1e000000"),
    (28, "ScanAutoCleanQuarantine",                   256, "79657300"),
    (29, "ScanAutoCleanQuarantineDays",               259, "1e000000"),
    (30, "ScanScheduleEnabled",                       256, "79657300"),
    (31, "ScanSkipLargeFiles",                        256, "79657300"),
    (32, "ScanCleanCookies",                          256, "79657300"),
    (33, "ScanLastScanTime",                          263, "00000000000000000000000000000000"),
    (34, "ScanLastDefinitionRemindTime",              263, "ea070300010009000f00120010006f00"),
    (35, "ScanRemindCheckForDefinitionUpdates",       256, "6e6f00"),
    (36, "ScanShowBalloonUpdateStatus ",              256, "6e6f00"),
    (37, "ScanRemindCheckForDefinitionUpdatesDays",   259, "05000000"),
    (38, "ScanMinFileSize",                           259, "00004000"),
    (39, "ScanOnlyKnownFileTypes",                    256, "79657300"),
    (40, "ScanIgnoreNonExecutableFiles",              256, "79657300"),
    (41, "ScanIgnoreSystemRestore",                   256, "6e6f00"),
    (42, "ScanShowIconInSystemTray",                  256, "79657300"),
    (43, "ScanKeepLogs",                              256, "79657300"),
    (44, "ScanKeepCleanLogs",                         256, "79657300"),
    (45, "ScanCustomMemory",                          256, "79657300"),
    (46, "ScanCustomRegistry",                        256, "79657300"),
    (47, "ScanCustomStartup",                         256, "79657300"),
    (48, "ScanCustomFolders",                         256, "79657300"),
    (49, "ScanCustomCookies",                         256, "79657300"),
    (50, "ScanAutoScanType",                          259, "03000000"),
    (51, "ScanAutoScanCheckForUpdates",               256, "79657300"),
    (52, "ScanAutoScanHideUserInterface",             256, "6e6f00"),
    (53, "ScanScheduleCheckForUpdates",               256, "6e6f00"),
    (54, "ScanCloseBrowsers",                         256, "6e6f00"),
    (55, "ScanClearTemp",                             256, "6e6f00"),
    (56, "ScanResolveLinks",                          256, "79657300"),
    (57, "ScanTerminateMemoryThreats",                256, "6e6f00"),
    (58, "ScanUseKernelFileDirect",                   256, "79657300"),
    (59, "ScanUseKernelRegistryDirect",               256, "79657300"),
    (60, "ScanUseDirectDiskAccess",                   256, "79657300"),
    (61, "ScanADS",                                   256, "79657300"),
    (62, "ScanDisplayContextMenu",                    256, "79657300"),
    (63, "ScanBoostActive",                           256, "79657300"),
    (64, "ScanBoostLevel",                            259, "feffffff"),
    (65, "ScanUnwanted",                              256, "79657300"),
    (66, "ScanModifiedFilesOnly",                     256, "6e6f00"),
    (67, "ScanModifiedFilesDays",                     259, "1e000000"),
    (68, "ScanArchiveFlags",                          259, "00000000"),
    (69, "DoNotShowSOSToaster",                       256, "6e6f00"),
    (70, "GameModeDuration",                          259, "ffffffff"),
];

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

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

    log::info!("Applied {count} SAS settings from reference configuration");
    Ok(count)
}

/// XML template for the SAS definition-update task (daily, repeating every 8 hours).
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

/// XML template for the SAS quick-scan task (Monday + Thursday at 14:00).
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
        .args(["-NoProfile", "-Command", "([System.Security.Principal.WindowsIdentity]::GetCurrent()).User.Value"])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to get current user SID"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn get_sas_db_path() -> anyhow::Result<PathBuf> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| anyhow::anyhow!("APPDATA environment variable not set"))?;
    let db_path = PathBuf::from(appdata)
        .join("SUPERAntiSpyware.com")
        .join("SUPERAntiSpyware")
        .join("SAS_CURRENTUSER.DB3");

    if !db_path.exists() {
        return Err(anyhow::anyhow!("SAS database not found at {}", db_path.display()));
    }

    Ok(db_path)
}

fn now_iso8601() -> String {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Date -Format 'yyyy-MM-ddTHH:mm:ss'"])
        .output()
        .ok();

    output
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "2026-01-01T14:00:00".to_string())
}

/// Configures SAS scheduled tasks by inserting entries into the SAS database
/// and registering Windows Task Scheduler tasks.
///
/// Returns (update_guid, scan_guid) on success.
pub fn configure_sas_scheduled_tasks() -> anyhow::Result<(String, String)> {
    let db_path = get_sas_db_path()?;
    let user_sid = get_current_user_sid()?;
    let start_time = now_iso8601();

    log::info!("SAS DB: {}", db_path.display());
    log::info!("User SID: {user_sid}");
    log::info!("Start time: {start_time}");

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

    // Apply all reference settings (splash screen, notifications, scan config, etc.)
    let settings_count = apply_sas_settings(&conn)?;
    log::info!("Wrote {settings_count} settings to SAS database");

    // Clear any existing scheduled tasks so we don't create duplicates
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
                "no",   // restart
                "no",   // shutdown
                task.clean,
                task.hide,
                task.updatebefore,
                task.runifmissed,
                &start_time,
                "no",   // disabled
            ],
        )?;
        log::info!("Inserted SAS task: type={} guid={}", task.task_type, task.guid);
    }

    drop(conn);

    // Register each task with Windows Task Scheduler
    let temp_dir = std::env::temp_dir();

    for task in &tasks {
        let xml_content = task.xml_template
            .replace("{TASK_GUID}", &task.guid)
            .replace("{USER_SID}", &user_sid)
            .replace("{START_TIME}", &start_time);

        let xml_path = temp_dir.join(format!("sas_task_{}.xml", task.guid));
        // Write as UTF-16 LE with BOM (required by schtasks for UTF-16 encoded XML)
        let mut utf16_bytes = vec![0xFF, 0xFE]; // BOM
        for code_unit in xml_content.encode_utf16() {
            utf16_bytes.extend_from_slice(&code_unit.to_le_bytes());
        }
        std::fs::write(&xml_path, &utf16_bytes)?;

        let task_name = format!(
            "\\SUPERAntiSpyware\\SUPERAntiSpyware Scheduled Task {}",
            task.guid
        );

        // Create the parent folder first (schtasks needs it)
        let _ = Command::new("schtasks")
            .args(["/Create", "/TN", "\\SUPERAntiSpyware\\placeholder", "/SC", "ONCE", "/ST", "00:00", "/TR", "cmd /c echo noop", "/F"])
            .output();
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", "\\SUPERAntiSpyware\\placeholder", "/F"])
            .output();

        let output = Command::new("schtasks")
            .args(["/Create", "/XML", &xml_path.to_string_lossy(), "/TN", &task_name, "/F"])
            .output()?;

        if output.status.success() {
            log::info!("Registered task: {task_name}");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("Failed to register task {task_name}: {stderr}");
        }

        let _ = std::fs::remove_file(&xml_path);
    }

    Ok((tasks[0].guid.clone(), tasks[1].guid.clone()))
}

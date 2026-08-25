//! WER LocalDumps registration for this executable.
//!
//! Without a `LocalDumps` key, WER keeps only the metadata half of an
//! `AppCrash_*` report and writes no `.mdmp`, so a crash cannot be debugged
//! afterwards. The key is applied on machines whose `computer` row is flagged
//! `is_internal` and removed on every other machine, so a bench box that gets
//! sold sheds it on the next launch.

#[cfg(target_os = "windows")]
const LOCAL_DUMPS_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps";

/// Where WER writes dumps for this executable.
#[cfg(target_os = "windows")]
pub const DUMP_FOLDER: &str = r"C:\ProgramData\Mastertech\CrashDumps";

/// `DumpType = 2` — a full dump rather than a mini or custom one.
#[cfg(target_os = "windows")]
const DUMP_TYPE_FULL: u32 = 2;

/// Dumps WER retains before it starts refusing to write new ones.
#[cfg(target_os = "windows")]
const DUMP_COUNT: u32 = 10;

/// Forces the policy on (`1`) or off (`0`) regardless of the internal flag.
#[cfg(target_os = "windows")]
const OVERRIDE_ENV: &str = "MTECH_LOCAL_DUMPS";

/// This executable's file name, e.g. `MasterTech.exe`.
#[cfg(target_os = "windows")]
fn image_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "MasterTech.exe".to_string())
}

/// Writes the LocalDumps key for this executable. Idempotent.
#[cfg(target_os = "windows")]
pub fn enable_local_dumps() -> Result<String, String> {
    let image = image_name();
    let path = format!(r"{LOCAL_DUMPS_KEY}\{image}");

    if let Err(e) = std::fs::create_dir_all(DUMP_FOLDER) {
        log::warn!("local dumps: {DUMP_FOLDER} not created ({e}); WER will create it on first crash");
    }

    let key = windows_registry::LOCAL_MACHINE
        .options()
        .read()
        .write()
        .create()
        .open(&path)
        .map_err(|e| format!("open {path}: {e}"))?;

    key.set_expand_string("DumpFolder", DUMP_FOLDER)
        .map_err(|e| format!("DumpFolder: {e}"))?;
    key.set_u32("DumpType", DUMP_TYPE_FULL)
        .map_err(|e| format!("DumpType: {e}"))?;
    key.set_u32("DumpCount", DUMP_COUNT)
        .map_err(|e| format!("DumpCount: {e}"))?;

    Ok(format!("{image}: full dumps -> {DUMP_FOLDER} (keep {DUMP_COUNT})"))
}

/// Removes this executable's LocalDumps key. Absent is success.
#[cfg(target_os = "windows")]
pub fn disable_local_dumps() -> Result<String, String> {
    let image = image_name();
    let parent = windows_registry::LOCAL_MACHINE
        .options()
        .read()
        .write()
        .open(LOCAL_DUMPS_KEY)
        .map_err(|e| format!("open {LOCAL_DUMPS_KEY}: {e}"))?;

    match parent.remove_tree(&image) {
        Ok(()) => Ok(format!("{image}: LocalDumps key removed")),
        Err(e) => Err(format!("remove {image}: {e}")),
    }
}

/// True when this executable already has a LocalDumps key.
#[cfg(target_os = "windows")]
pub fn local_dumps_enabled() -> bool {
    windows_registry::LOCAL_MACHINE
        .options()
        .read()
        .open(format!(r"{LOCAL_DUMPS_KEY}\{}", image_name()))
        .is_ok()
}

/// Retries [`apply_policy_for_this_machine`] until the database answers.
#[cfg(target_os = "windows")]
pub async fn apply_policy_when_ready() {
    const ATTEMPTS: u32 = 12;
    const GAP: std::time::Duration = std::time::Duration::from_secs(10);
    for attempt in 1..=ATTEMPTS {
        if apply_policy_for_this_machine().await {
            return;
        }
        if attempt < ATTEMPTS {
            tokio::time::sleep(GAP).await;
        }
    }
    log::warn!("local dumps: internal flag never resolved; key left as-is");
}

#[cfg(not(target_os = "windows"))]
pub async fn apply_policy_when_ready() {}

/// Applies the LocalDumps policy for this machine: on when its `computer` row
/// is flagged internal or [`OVERRIDE_ENV`] forces it, off otherwise. Returns
/// false when the flag could not be resolved.
#[cfg(target_os = "windows")]
pub async fn apply_policy_for_this_machine() -> bool {
    let wanted = match std::env::var(OVERRIDE_ENV).ok().as_deref() {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    };

    let wanted = match wanted {
        Some(forced) => {
            log::info!("local dumps: {OVERRIDE_ENV} forces enabled={forced}");
            forced
        }
        None => {
            let cs = crate::filesystem::get_client_hash().connection_string.clone();
            match database::schema::outcome::internal_computer_for_client(&cs).await {
                Ok(found) => found.is_some(),
                Err(e) => {
                    log::debug!("local dumps: internal-flag lookup for {cs} failed: {e}");
                    return false;
                }
            }
        }
    };

    if wanted == local_dumps_enabled() {
        return true;
    }

    let outcome = if wanted {
        enable_local_dumps()
    } else {
        disable_local_dumps()
    };
    match outcome {
        Ok(msg) => log::info!("local dumps: {msg}"),
        Err(e) => log::warn!("local dumps: {e}"),
    }
    true
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn image_name_is_an_exe_file_name() {
        let name = image_name();
        assert!(!name.contains('\\'), "{name} still carries a path");
        assert!(name.to_ascii_lowercase().ends_with(".exe"), "{name}");
    }
}

//! Installed-program enumeration + uninstall for slice 3 of the
//! connected-client refactor.
//!
//! The old version of this module called the Win32 registry API
//! directly to return a bare `Vec<String>` of display names. That
//! was enough for the original use case but the admin's new
//! Installed Programs viewer needs the full registry row (version,
//! publisher, uninstall string, etc.) so it can render a table and
//! support a one-click uninstall.
//!
//! We switched the enumeration path from `windows::Win32::System::Registry`
//! to a PowerShell `Get-ItemProperty` walk for two reasons:
//!
//!   1. Same pattern as `gather_security_inventory`'s registry
//!      backstop — one PS launch returns every row as JSON, no
//!      hand-marshaled `Vec<u16>` buffers. Cheaper to maintain.
//!   2. PowerShell handles registry redirection (Wow6432Node) and
//!      the `HKCU` walk in one pass without needing a separate
//!      `KEY_WOW64_32KEY` open.
//!
//! Uninstalls go through `cmd /C`, capture the exit code, and
//! return a structured result. See `run_uninstall` for the
//! strategy ladder.

use displays::InstalledProgram;

/// Walks HKLM, HKLM\WOW6432Node, and HKCU's `Uninstall` subtrees
/// and returns one [`InstalledProgram`] per registry row that has
/// a `DisplayName`. Rows without a `DisplayName` are skipped —
/// those are usually update-package stubs that the user can't
/// meaningfully uninstall from the admin viewer.
pub async fn list_installed_programs() -> Vec<InstalledProgram> {
    // Each PowerShell hashtable carries:
    //   - `Id`              — registry subkey name (canonical id)
    //   - `Name`            — DisplayName, falling back to Id
    //   - `Version`         — DisplayVersion
    //   - `Publisher`       — Publisher
    //   - `InstallDate`     — InstallDate (YYYYMMDD per convention)
    //   - `EstimatedSize`   — EstimatedSize (KiB)
    //   - `UninstallString` — raw uninstall command
    //   - `QuietUninstall`  — QuietUninstallString
    //   - `Hive`            — "HKLM" / "HKLM-Wow6432" / "HKCU"
    //   - `IsWow6432`       — 0/1
    let ps_cmd = r#"
$paths = @(
  @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'; Hive = 'HKLM'; IsWow = 0 },
  @{ Path = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'; Hive = 'HKLM-Wow6432'; IsWow = 1 },
  @{ Path = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'; Hive = 'HKCU'; IsWow = 0 }
)
$results = foreach ($entry in $paths) {
  if (Test-Path $entry.Path) {
    Get-ChildItem $entry.Path -ErrorAction SilentlyContinue | ForEach-Object {
      $subkey = $_.PSChildName
      $props  = $_ | Get-ItemProperty -ErrorAction SilentlyContinue
      if ($props.DisplayName) {
        [PSCustomObject]@{
          Id              = $subkey
          Name            = $props.DisplayName
          Version         = $props.DisplayVersion
          Publisher       = $props.Publisher
          InstallDate     = $props.InstallDate
          EstimatedSize   = $props.EstimatedSize
          UninstallString = $props.UninstallString
          QuietUninstall  = $props.QuietUninstallString
          Hive            = $entry.Hive
          IsWow6432       = $entry.IsWow
        }
      }
    }
  }
}
$results | ConvertTo-Json -Compress -Depth 3
"#;

    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_cmd])
        .output()
        .await;

    let Ok(out) = output else {
        log::warn!("list_installed_programs: PowerShell launch failed");
        return Vec::new();
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("list_installed_programs: JSON parse failed: {e}");
            return Vec::new();
        }
    };
    // `ConvertTo-Json` collapses single-element arrays — accept both.
    let rows: Vec<&serde_json::Value> = match &parsed {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(_) => vec![&parsed],
        _ => return Vec::new(),
    };

    rows.into_iter()
        .filter_map(|v| {
            let id = v.get("Id")?.as_str()?.to_string();
            let name = v
                .get("Name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| id.clone());
            let registry_hive = v
                .get("Hive")
                .and_then(|x| x.as_str())
                .unwrap_or("HKLM")
                .to_string();
            let is_wow6432 = v
                .get("IsWow6432")
                .and_then(|x| x.as_u64())
                .map(|n| n != 0)
                .unwrap_or(false);
            Some(InstalledProgram {
                id,
                name,
                version: v
                    .get("Version")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                publisher: v
                    .get("Publisher")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                install_date: v
                    .get("InstallDate")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                estimated_size_kb: v.get("EstimatedSize").and_then(|x| x.as_u64()),
                uninstall_string: v
                    .get("UninstallString")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                quiet_uninstall_string: v
                    .get("QuietUninstall")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                registry_hive,
                is_wow6432,
            })
        })
        .collect()
}

/// What we actually executed, for the result message back to the
/// admin. The order of the variants reflects the ladder
/// `try_silent_uninstall` walks down.
#[derive(Debug)]
enum UninstallStrategy {
    /// Publisher already wrote a `QuietUninstallString` —
    /// preferred path when `prefer_silent` is true.
    QuietRegistry,
    /// MSI `/X{GUID} /qn` synthesized from an `MsiExec.exe /I{GUID}`
    /// uninstall command.
    MsiSilent,
    /// Heuristic silent switch appended to `UninstallString` —
    /// `/S` for NSIS, `/SILENT /NORESTART` for InnoSetup-style
    /// `unins000.exe`. Field is debug-only (it's surfaced in the
    /// `log::info!` line before we run the command) — `dead_code`
    /// silences the warning since reading it via the derived
    /// `Debug` doesn't count.
    HeuristicSilent {
        #[allow(dead_code)]
        switch: &'static str,
    },
    /// Last resort: the raw `UninstallString`, which usually
    /// surfaces the publisher's GUI uninstaller on the remote.
    Raw,
}

/// Run an uninstall and return `(success, summary_message)`. The
/// message describes which strategy fired so the admin can see
/// "Uninstalled via MSI silent" vs "Uninstalled via raw command",
/// which matters when triaging which products can be unattended
/// from the admin console.
pub async fn run_uninstall(program: &InstalledProgram, prefer_silent: bool) -> (bool, String) {
    // Pick the command + record which strategy we used.
    let (cmd, strategy) = match choose_command(program, prefer_silent) {
        Some(pair) => pair,
        None => {
            return (
                false,
                "No UninstallString registered for this program; nothing to run.".to_string(),
            );
        }
    };

    log::info!(
        "Uninstalling '{}' via {:?}: `{}`",
        program.name,
        strategy,
        cmd
    );

    // Shell out via `cmd /C` so quoting in the registered
    // uninstall command is parsed by the same rules the publisher
    // assumed when they wrote it.
    let output = tokio::process::Command::new("cmd")
        .args(["/C", &cmd])
        .output()
        .await;

    match output {
        Ok(out) => {
            let success = out.status.success();
            let strategy_label = strategy_label(&strategy);
            let exit = out.status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
            let stderr_snippet = String::from_utf8_lossy(&out.stderr);
            let stderr_trim = stderr_snippet.trim();
            let mut message = format!("Strategy: {strategy_label} (exit {exit})");
            if !success && !stderr_trim.is_empty() {
                // Truncate stderr so a chatty uninstaller doesn't
                // flood the admin's status toast.
                let snippet = stderr_trim.chars().take(240).collect::<String>();
                message.push_str(" — ");
                message.push_str(&snippet);
            }
            (success, message)
        }
        Err(e) => (false, format!("Spawn failed: {e}")),
    }
}

fn strategy_label(s: &UninstallStrategy) -> &'static str {
    match s {
        UninstallStrategy::QuietRegistry => "QuietUninstallString",
        UninstallStrategy::MsiSilent => "MSI silent (/X /qn)",
        UninstallStrategy::HeuristicSilent { switch: _ } => "Heuristic silent",
        UninstallStrategy::Raw => "Raw UninstallString",
    }
}

/// Picks the command line to execute. Returns `None` if no
/// uninstall information is registered at all.
fn choose_command(
    program: &InstalledProgram,
    prefer_silent: bool,
) -> Option<(String, UninstallStrategy)> {
    let raw = program.uninstall_string.as_deref().map(str::trim);
    let quiet = program.quiet_uninstall_string.as_deref().map(str::trim);

    if prefer_silent {
        // 1. Publisher-blessed quiet command.
        if let Some(q) = quiet.filter(|s| !s.is_empty()) {
            return Some((q.to_string(), UninstallStrategy::QuietRegistry));
        }

        // 2. MSI rewrite. The uninstall command for MSI products
        // is almost always `MsiExec.exe /I{GUID}` or
        // `MsiExec.exe /X{GUID}`. We rewrite to `/X{GUID} /qn` so
        // it runs silently. Match case-insensitively because the
        // registered case varies.
        if let Some(r) = raw {
            if let Some(synthesized) = msi_silent(r) {
                return Some((synthesized, UninstallStrategy::MsiSilent));
            }
        }

        // 3. Heuristic silent switch — InnoSetup (`unins???.exe`)
        // and NSIS installers tend to support these. We detect
        // them by filename / pattern rather than digging through
        // executable resources.
        if let Some(r) = raw {
            let lower = r.to_lowercase();
            if lower.contains("unins") && lower.contains(".exe") {
                // InnoSetup
                let cmd = format!("{r} /SILENT /NORESTART");
                return Some((
                    cmd,
                    UninstallStrategy::HeuristicSilent {
                        switch: "/SILENT /NORESTART",
                    },
                ));
            }
            if lower.ends_with("uninstall.exe") || lower.contains("nsis") {
                // NSIS — single-character switch.
                let cmd = format!("{r} /S");
                return Some((cmd, UninstallStrategy::HeuristicSilent { switch: "/S" }));
            }
        }
    }

    // 4. Raw fallback.
    raw.filter(|s| !s.is_empty())
        .map(|r| (r.to_string(), UninstallStrategy::Raw))
}

/// If the input looks like `MsiExec.exe /I{GUID}` or
/// `MsiExec.exe /X{GUID}` (case-insensitive, optional quoting),
/// returns the equivalent `/X{GUID} /qn` silent variant.
fn msi_silent(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    if !lower.contains("msiexec") {
        return None;
    }
    // Find the `/I{` or `/X{` flag and the closing brace; rewrite
    // to `/X{…} /qn`. The GUID may or may not be quoted; we
    // preserve the substring as-is rather than parsing it out.
    let flag_start = lower
        .find("/i{")
        .or_else(|| lower.find("/x{"))
        .or_else(|| lower.find("/i {"))
        .or_else(|| lower.find("/x {"))?;
    let open_brace = raw[flag_start..].find('{')? + flag_start;
    let close_brace = raw[open_brace..].find('}')? + open_brace;
    let guid_with_braces = &raw[open_brace..=close_brace];
    Some(format!("MsiExec.exe /X{guid_with_braces} /qn /norestart"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msi_silent_rewrites_install_flag() {
        let raw = r#"MsiExec.exe /I{C2C7E2E6-1234-5678-ABCD-1234567890AB}"#;
        let out = msi_silent(raw).expect("should detect MSI");
        assert!(out.contains("/X{C2C7E2E6-1234-5678-ABCD-1234567890AB}"));
        assert!(out.contains("/qn"));
    }

    #[test]
    fn msi_silent_preserves_uninstall_flag() {
        let raw = r#"MsiExec.exe /X{C2C7E2E6-1234-5678-ABCD-1234567890AB}"#;
        let out = msi_silent(raw).expect("should detect MSI");
        assert!(out.contains("/X{C2C7E2E6-1234-5678-ABCD-1234567890AB}"));
        assert!(out.contains("/qn"));
    }

    #[test]
    fn msi_silent_ignores_non_msi() {
        assert!(msi_silent(r#""C:\Program Files\Foo\unins000.exe""#).is_none());
    }

    #[test]
    fn choose_command_prefers_quiet_registry() {
        let p = InstalledProgram {
            id: "k".into(),
            name: "K".into(),
            version: None,
            publisher: None,
            install_date: None,
            estimated_size_kb: None,
            uninstall_string: Some("loud.exe".into()),
            quiet_uninstall_string: Some("quiet.exe".into()),
            registry_hive: "HKLM".into(),
            is_wow6432: false,
        };
        let (cmd, _) = choose_command(&p, true).unwrap();
        assert_eq!(cmd, "quiet.exe");
    }

    #[test]
    fn choose_command_falls_back_to_raw_when_not_silent() {
        let p = InstalledProgram {
            id: "k".into(),
            name: "K".into(),
            version: None,
            publisher: None,
            install_date: None,
            estimated_size_kb: None,
            uninstall_string: Some("loud.exe".into()),
            quiet_uninstall_string: Some("quiet.exe".into()),
            registry_hive: "HKLM".into(),
            is_wow6432: false,
        };
        let (cmd, _) = choose_command(&p, false).unwrap();
        assert_eq!(cmd, "loud.exe");
    }

    #[test]
    fn choose_command_detects_innosetup_silent() {
        let p = InstalledProgram {
            id: "k".into(),
            name: "K".into(),
            version: None,
            publisher: None,
            install_date: None,
            estimated_size_kb: None,
            uninstall_string: Some(r#""C:\Program Files\Foo\unins000.exe""#.into()),
            quiet_uninstall_string: None,
            registry_hive: "HKLM".into(),
            is_wow6432: false,
        };
        let (cmd, _) = choose_command(&p, true).unwrap();
        assert!(cmd.contains("/SILENT /NORESTART"));
    }
}

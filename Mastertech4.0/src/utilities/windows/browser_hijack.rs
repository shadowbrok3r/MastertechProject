//! Browser hijack detection and cleanup.
//!
//! Covers the four places a hijacker survives an uninstall: enterprise policy
//! keys, URL arguments appended to browser shortcuts, autostart entries that
//! open a browser at a URL, and per-profile homepage / startup / search
//! overrides. Scanning is read-only; cleanup touches only the first three and
//! leaves profile preferences to the tech.

use std::path::{Path, PathBuf};
use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE};

/// Where a finding came from, which decides whether cleanup can act on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// Chromium policy value under `Software\Policies\...`.
    Policy,
    /// Browser shortcut carrying arguments.
    Shortcut,
    /// Run / RunOnce value launching a browser at a URL.
    Autostart,
    /// Homepage, startup URL or search engine inside a browser profile.
    ProfilePref,
}

impl FindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Shortcut => "shortcut",
            Self::Autostart => "autostart",
            Self::ProfilePref => "profile",
        }
    }

    /// Profile preferences are reported only — rewriting a live profile risks
    /// corrupting it, and the browser restores its own on next launch.
    pub fn is_removable(self) -> bool {
        !matches!(self, Self::ProfilePref)
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub kind: FindingKind,
    /// Registry path, file path or profile file the finding sits in.
    pub location: String,
    /// The offending value, trimmed for display.
    pub detail: String,
}

impl Finding {
    fn new(kind: FindingKind, location: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail: String = detail.into();
        Self {
            kind,
            location: location.into(),
            detail: truncate(&detail, 200),
        }
    }

    pub fn line(&self) -> String {
        format!("[{}] {} -> {}", self.kind.as_str(), self.location, self.detail)
    }
}

/// Chromium policy roots, per browser.
const POLICY_PATHS: &[&str] = &[
    r"Software\Policies\Google\Chrome",
    r"Software\Policies\Microsoft\Edge",
    r"Software\Policies\BraveSoftware\Brave",
    r"Software\Policies\Chromium",
];

/// Policy values a hijacker sets to pin the homepage, new tab, startup set or
/// default search engine.
const POLICY_VALUES: &[&str] = &[
    "HomepageLocation",
    "HomepageIsNewTabPage",
    "NewTabPageLocation",
    "RestoreOnStartup",
    "DefaultSearchProviderEnabled",
    "DefaultSearchProviderName",
    "DefaultSearchProviderSearchURL",
    "DefaultSearchProviderSuggestURL",
    "DefaultSearchProviderKeyword",
];

/// Policy subkeys whose numbered values carry the payload.
const POLICY_SUBKEYS: &[&str] = &["RestoreOnStartupURLs", "ExtensionInstallForcelist"];

const AUTOSTART_PATHS: &[&str] = &[
    r"Software\Microsoft\Windows\CurrentVersion\Run",
    r"Software\Microsoft\Windows\CurrentVersion\RunOnce",
];

const BROWSER_EXES: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "brave.exe",
    "firefox.exe",
    "opera.exe",
    "iexplore.exe",
    "launcher.exe",
];

/// Chromium profile roots under `%LOCALAPPDATA%`, plus Firefox under
/// `%APPDATA%`.
const CHROMIUM_USER_DATA: &[&str] = &[
    r"Google\Chrome\User Data",
    r"Microsoft\Edge\User Data",
    r"BraveSoftware\Brave-Browser\User Data",
    r"Chromium\User Data",
];

/// Read-only sweep of every hijack surface.
pub fn scan() -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(scan_policies());
    findings.extend(scan_autostart());
    findings.extend(scan_shortcuts());
    findings.extend(scan_profiles());
    findings
}

/// Removes what [`scan`] found in the policy, autostart and shortcut surfaces.
/// Returns one line per action for the script log.
pub fn remediate() -> Vec<String> {
    let mut actions = Vec::new();
    actions.extend(clear_policies());
    actions.extend(clear_autostart());
    actions.extend(clean_shortcuts());

    let remaining: Vec<Finding> = scan_profiles();
    for finding in remaining {
        actions.push(format!("review by hand: {}", finding.line()));
    }
    actions
}

fn hives() -> [(&'static str, &'static Key); 2] {
    [("HKCU", CURRENT_USER), ("HKLM", LOCAL_MACHINE)]
}

fn scan_policies() -> Vec<Finding> {
    let mut findings = Vec::new();
    for (hive_name, hive) in hives() {
        for path in POLICY_PATHS {
            let Ok(key) = hive.open(path) else { continue };
            for value in POLICY_VALUES {
                if let Some(data) = read_display_value(&key, value) {
                    findings.push(Finding::new(
                        FindingKind::Policy,
                        format!(r"{hive_name}\{path}\{value}"),
                        data,
                    ));
                }
            }
            for sub in POLICY_SUBKEYS {
                let Ok(subkey) = key.open(sub) else { continue };
                let Ok(values) = subkey.values() else { continue };
                for (name, _) in values {
                    if let Some(data) = read_display_value(&subkey, &name) {
                        findings.push(Finding::new(
                            FindingKind::Policy,
                            format!(r"{hive_name}\{path}\{sub}\{name}"),
                            data,
                        ));
                    }
                }
            }
        }
    }
    findings
}

fn clear_policies() -> Vec<String> {
    let mut actions = Vec::new();
    for (hive_name, hive) in hives() {
        for path in POLICY_PATHS {
            let Ok(key) = hive.options().read().write().open(path) else { continue };
            for value in POLICY_VALUES {
                if key.get_value(value).is_err() {
                    continue;
                }
                match key.remove_value(value) {
                    Ok(_) => actions.push(format!(r"removed policy {hive_name}\{path}\{value}")),
                    Err(e) => actions.push(format!(r"could not remove {hive_name}\{path}\{value}: {e}")),
                }
            }
            for sub in POLICY_SUBKEYS {
                let Ok(subkey) = key.options().read().write().open(sub) else { continue };
                let Ok(values) = subkey.values() else { continue };
                let names: Vec<String> = values.map(|(name, _)| name).collect();
                for name in names {
                    match subkey.remove_value(&name) {
                        Ok(_) => actions.push(format!(r"removed policy {hive_name}\{path}\{sub}\{name}")),
                        Err(e) => actions.push(format!(r"could not remove {hive_name}\{path}\{sub}\{name}: {e}")),
                    }
                }
            }
        }
    }
    actions
}

fn scan_autostart() -> Vec<Finding> {
    let mut findings = Vec::new();
    for (hive_name, hive) in hives() {
        for path in AUTOSTART_PATHS {
            let Ok(key) = hive.open(path) else { continue };
            let Ok(values) = key.values() else { continue };
            for (name, _) in values {
                let Ok(data) = key.get_string(&name) else { continue };
                if launches_browser_at_url(&data) {
                    findings.push(Finding::new(
                        FindingKind::Autostart,
                        format!(r"{hive_name}\{path}\{name}"),
                        data,
                    ));
                }
            }
        }
    }
    findings
}

fn clear_autostart() -> Vec<String> {
    let mut actions = Vec::new();
    for (hive_name, hive) in hives() {
        for path in AUTOSTART_PATHS {
            let Ok(key) = hive.options().read().write().open(path) else { continue };
            let Ok(values) = key.values() else { continue };
            let hijacks: Vec<String> = values
                .filter_map(|(name, _)| {
                    let data = key.get_string(&name).ok()?;
                    launches_browser_at_url(&data).then_some(name)
                })
                .collect();
            for name in hijacks {
                match key.remove_value(&name) {
                    Ok(_) => actions.push(format!(r"removed autostart {hive_name}\{path}\{name}")),
                    Err(e) => actions.push(format!(r"could not remove {hive_name}\{path}\{name}: {e}")),
                }
            }
        }
    }
    actions
}

/// A command line that runs a browser and hands it a URL or an extension to
/// side-load. A bare browser path is normal and does not match.
fn launches_browser_at_url(command: &str) -> bool {
    let lower = command.to_lowercase();
    if !BROWSER_EXES.iter().any(|exe| lower.contains(exe)) {
        return false;
    }
    // The target itself ends at the exe; anything after it is an argument.
    let Some(cut) = BROWSER_EXES
        .iter()
        .filter_map(|exe| lower.find(exe).map(|i| i + exe.len()))
        .max()
    else {
        return false;
    };
    let args = lower[cut..].trim_matches(|c| c == '"' || c == ' ');
    args.contains("http://")
        || args.contains("https://")
        || args.contains("--app=")
        || args.contains("--load-extension")
        || args.contains("--homepage")
}

fn scan_shortcuts() -> Vec<Finding> {
    shortcut_arguments()
        .into_iter()
        .map(|(path, args)| Finding::new(FindingKind::Shortcut, path, args))
        .collect()
}

fn clean_shortcuts() -> Vec<String> {
    let hijacked: Vec<String> = shortcut_arguments().into_iter().map(|(p, _)| p).collect();
    if hijacked.is_empty() {
        return Vec::new();
    }

    let mut actions = Vec::new();
    for path in hijacked {
        let script = format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); \
             $s.Arguments = ''; $s.Save()",
            path.replace('\'', "''")
        );
        match run_powershell(&script) {
            Ok(_) => actions.push(format!("cleared arguments on {path}")),
            Err(e) => actions.push(format!("could not clean {path}: {e}")),
        }
    }
    actions
}

/// Every browser shortcut in the usual locations that carries arguments,
/// as `(path, arguments)`.
fn shortcut_arguments() -> Vec<(String, String)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["USERPROFILE", "PUBLIC", "APPDATA", "ProgramData"] {
        let Ok(base) = std::env::var(var) else { continue };
        let base = PathBuf::from(base);
        roots.push(base.join("Desktop"));
        roots.push(base.join(r"Microsoft\Windows\Start Menu"));
        roots.push(base.join(r"Microsoft\Internet Explorer\Quick Launch"));
    }
    roots.retain(|p| p.exists());
    if roots.is_empty() {
        return Vec::new();
    }

    let list: Vec<String> = roots
        .iter()
        .map(|p| format!("'{}'", p.to_string_lossy().replace('\'', "''")))
        .collect();
    let script = format!(
        "$sh = New-Object -ComObject WScript.Shell; \
         Get-ChildItem -Path {} -Filter *.lnk -Recurse -ErrorAction SilentlyContinue | ForEach-Object {{ \
             $lnk = $sh.CreateShortcut($_.FullName); \
             if ($lnk.Arguments.Trim()) {{ $_.FullName + '|' + $lnk.TargetPath + '|' + $lnk.Arguments }} \
         }}",
        list.join(",")
    );

    let Ok(output) = run_powershell(&script) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(3, '|');
            let path = parts.next()?.trim().to_string();
            let target = parts.next()?.trim().to_lowercase();
            let args = parts.next()?.trim().to_string();
            if path.is_empty() || args.is_empty() {
                return None;
            }
            if !BROWSER_EXES.iter().any(|exe| target.ends_with(exe)) {
                return None;
            }
            // Chromium's own installer writes --profile-directory on profile
            // shortcuts; only URLs and side-loads are hijacks.
            let lower = args.to_lowercase();
            let suspicious = lower.contains("http://")
                || lower.contains("https://")
                || lower.contains("--app=")
                || lower.contains("--load-extension")
                || lower.contains("--homepage");
            suspicious.then_some((path, args))
        })
        .collect()
}

fn scan_profiles() -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        for user_data in CHROMIUM_USER_DATA {
            let root = Path::new(&local).join(user_data);
            if !root.exists() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&root) else { continue };
            for entry in entries.flatten() {
                let prefs = entry.path().join("Preferences");
                if prefs.exists() {
                    findings.extend(chromium_profile_findings(&prefs));
                }
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let profiles = Path::new(&appdata).join(r"Mozilla\Firefox\Profiles");
        if let Ok(entries) = std::fs::read_dir(&profiles) {
            for entry in entries.flatten() {
                for name in ["prefs.js", "user.js"] {
                    let file = entry.path().join(name);
                    if file.exists() {
                        findings.extend(firefox_profile_findings(&file));
                    }
                }
            }
        }
    }
    findings
}

fn chromium_profile_findings(prefs: &Path) -> Vec<Finding> {
    let Ok(raw) = std::fs::read_to_string(prefs) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new() };
    let location = prefs.to_string_lossy().to_string();
    let mut findings = Vec::new();

    if let Some(home) = json.get("homepage").and_then(|v| v.as_str()) {
        if !home.trim().is_empty() {
            findings.push(Finding::new(FindingKind::ProfilePref, &location, format!("homepage = {home}")));
        }
    }
    if let Some(urls) = json
        .get("session")
        .and_then(|s| s.get("startup_urls"))
        .and_then(|v| v.as_array())
    {
        for url in urls.iter().filter_map(|u| u.as_str()) {
            findings.push(Finding::new(FindingKind::ProfilePref, &location, format!("startup url = {url}")));
        }
    }
    if let Some(search) = json
        .get("default_search_provider_data")
        .and_then(|d| d.get("template_url_data"))
        .and_then(|t| t.get("url"))
        .and_then(|v| v.as_str())
    {
        if !is_known_search_engine(search) {
            findings.push(Finding::new(FindingKind::ProfilePref, &location, format!("search = {search}")));
        }
    }
    findings
}

fn firefox_profile_findings(prefs: &Path) -> Vec<Finding> {
    let Ok(raw) = std::fs::read_to_string(prefs) else { return Vec::new() };
    let location = prefs.to_string_lossy().to_string();
    raw.lines()
        .filter(|line| {
            line.contains("browser.startup.homepage")
                || line.contains("browser.newtabpage.pinned")
                || line.contains("keyword.URL")
        })
        .map(|line| Finding::new(FindingKind::ProfilePref, &location, line.trim()))
        .collect()
}

/// The engines a machine ships with; anything else replaced the default.
fn is_known_search_engine(url: &str) -> bool {
    let lower = url.to_lowercase();
    [
        "google.com",
        "bing.com",
        "duckduckgo.com",
        "yahoo.com",
        "ecosia.org",
        "search.brave.com",
    ]
    .iter()
    .any(|host| lower.contains(host))
}

fn read_display_value(key: &Key, name: &str) -> Option<String> {
    if let Ok(text) = key.get_string(name) {
        return (!text.trim().is_empty()).then_some(text);
    }
    key.get_u32(name).ok().map(|n| n.to_string())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn run_powershell(script: &str) -> anyhow::Result<String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(crate::filesystem::system_info::CREATE_NO_WINDOW)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_browser_path_is_not_a_hijack() {
        assert!(!launches_browser_at_url(
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#
        ));
    }

    #[test]
    fn a_browser_handed_a_url_is_a_hijack() {
        assert!(launches_browser_at_url(
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe" https://search.example"#
        ));
    }

    #[test]
    fn a_side_loaded_extension_is_a_hijack() {
        assert!(launches_browser_at_url(
            r#"C:\Windows\msedge.exe --load-extension=C:\ProgramData\thing"#
        ));
    }

    #[test]
    fn a_non_browser_autostart_is_ignored() {
        assert!(!launches_browser_at_url(r#"C:\Tools\updater.exe https://vendor.example"#));
    }

    #[test]
    fn the_stock_search_engines_are_not_findings() {
        assert!(is_known_search_engine("https://www.google.com/search?q={searchTerms}"));
        assert!(!is_known_search_engine("https://search.hijack.example/?q={searchTerms}"));
    }
}

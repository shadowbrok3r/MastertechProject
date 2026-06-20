//! Vendor-specific OEM provisioning steps (ported from QCWizard `Bimbox` /
//! `VRChat` tweaks): At-Home Support shortcut/bookmark removal and the VRChat
//! custom installer. The VRChat step is Windows-only; the rest are portable.

use std::path::Path;

/// Delete the At-Home Support desktop shortcut from the public and user
/// desktops. Missing shortcuts are skipped.
pub fn remove_at_home_support() -> anyhow::Result<String> {
    let mut targets = Vec::new();
    if let Ok(public) = std::env::var("PUBLIC") {
        targets.push(Path::new(&public).join("Desktop").join("At-Home Support.lnk"));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        targets.push(Path::new(&profile).join("Desktop").join("At-Home Support.lnk"));
    }

    let mut removed = 0usize;
    for path in &targets {
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow::anyhow!("remove {}: {e}", path.display())),
        }
    }
    Ok(format!("Removed {removed} At-Home Support shortcut(s)."))
}

/// Strip At-Home Support entries from the Edge bookmarks file, rewriting it
/// only when entries were removed.
pub fn remove_edge_favorites() -> anyhow::Result<String> {
    let local = std::env::var("LOCALAPPDATA")
        .map_err(|e| anyhow::anyhow!("LOCALAPPDATA unset: {e}"))?;
    let path = Path::new(&local)
        .join("Microsoft")
        .join("Edge")
        .join("User Data")
        .join("Default")
        .join("Bookmarks");
    if !path.exists() {
        return Ok("no Edge bookmarks file".into());
    }

    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;

    let removed = filter_bookmarks(&mut value);
    if removed > 0 {
        let out = serde_json::to_string_pretty(&value)?;
        std::fs::write(&path, out).map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    }
    Ok(format!("Removed {removed} At-Home Support bookmark(s)."))
}

/// Recursively drop children whose name contains "At-Home Support"
/// (case-insensitive) from every `children` array. Returns the count removed.
fn filter_bookmarks(value: &mut serde_json::Value) -> usize {
    let mut removed = 0;
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(children)) = map.get_mut("children") {
                let before = children.len();
                children.retain(|child| {
                    !child
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n.to_lowercase().contains("at-home support"))
                        .unwrap_or(false)
                });
                removed += before - children.len();
            }
            for child in map.values_mut() {
                removed += filter_bookmarks(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                removed += filter_bookmarks(item);
            }
        }
        _ => {}
    }
    removed
}

const CUSTOM_SHARE: &str = r"\\winbits7\Custom\VRChat";

/// Stage and run the VRChat custom installer from the share, passing the asset
/// tag. Returns the exit status line.
#[cfg(windows)]
pub fn install_vrchat_custom(asset_tag: &str) -> anyhow::Result<String> {
    use std::process::Command;

    let exe = std::fs::read_dir(CUSTOM_SHARE)
        .map_err(|e| anyhow::anyhow!("read {CUSTOM_SHARE}: {e}"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("no VRChat installer (*.exe) in {CUSTOM_SHARE}"))?;

    let file_name = exe
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("share path has no file name: {}", exe.display()))?;
    let dst = std::env::temp_dir().join(file_name);
    std::fs::copy(&exe, &dst).map_err(|e| anyhow::anyhow!("copy {} → temp: {e}", exe.display()))?;

    let status = Command::new(&dst)
        .arg(asset_tag)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn {}: {e}", dst.display()))?;
    if status.success() {
        Ok(format!("VRChat installer {file_name:?} completed ({status})."))
    } else {
        Err(anyhow::anyhow!("VRChat installer {file_name:?} exited with {status}"))
    }
}

#[cfg(not(windows))]
pub fn install_vrchat_custom(_asset_tag: &str) -> anyhow::Result<String> {
    Err(anyhow::anyhow!("VRChat installer is Windows-only"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filter_removes_at_home_support_keeps_others() {
        let mut value = json!({
            "roots": {
                "bookmark_bar": {
                    "children": [
                        {"name": "At-Home Support", "type": "url"},
                        {"name": "Keep Me", "type": "url"}
                    ]
                }
            }
        });
        let removed = filter_bookmarks(&mut value);
        assert_eq!(removed, 1);
        let children = value["roots"]["bookmark_bar"]["children"].as_array().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["name"], "Keep Me");
    }
}

//! Stage installer binaries from the `\\winbits7` share to a local cache
//! (ported from QCWizard `FileDownloading`). UNC copy over the LAN — no R2/HTTP.

use std::path::{Path, PathBuf};

/// Share root holding driver/software installers.
pub const SHARE_ROOT: &str = r"\\winbits7\copyfolder\Install Before Generalize";

/// Local cache directory for staged binaries.
pub fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("driver_cache"))
        .unwrap_or_else(|| std::env::temp_dir().join("mastertech_qc_driver_cache"))
}

/// Copy a share-relative path (e.g. `Intel/intel_chipset.exe`) into the cache,
/// reusing an existing copy when the byte size already matches.
pub fn stage_from_share(relative: &str) -> anyhow::Result<PathBuf> {
    let relative = relative.replace('/', "\\");
    let src = Path::new(SHARE_ROOT).join(&relative);
    let file_name = src
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("share path has no file name: {relative}"))?;
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let dst = dir.join(file_name);

    let src_len = std::fs::metadata(&src)
        .map_err(|e| anyhow::anyhow!("share file unavailable ({}): {e}", src.display()))?
        .len();
    if let Ok(meta) = std::fs::metadata(&dst) {
        if meta.len() == src_len {
            return Ok(dst); // already staged
        }
    }
    std::fs::copy(&src, &dst).map_err(|e| anyhow::anyhow!("copy {} → cache: {e}", src.display()))?;
    Ok(dst)
}

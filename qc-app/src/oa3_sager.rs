//! Builds inject/clear command text for Sager H2OOAE + `oa3tool`. Wrapper root and `.bin`
//! paths come from app settings (`h2ooae_exe`, `inject_command_line`, `clear_command_line`).

use std::path::{Path, PathBuf};

/// H2OOAE subfolder under the wrapper root (`H2O14`, `H2O12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum H2oGeneration {
    /// `H2O14\H2OOAE-Wx64.exe`
    H2O14,
    /// `H2O12\H2OOAE-Wx64.exe`
    H2O12,
}

impl H2oGeneration {
    pub fn all() -> &'static [H2oGeneration] {
        &[H2oGeneration::H2O14, H2oGeneration::H2O12]
    }

    pub fn label(self) -> &'static str {
        match self {
            H2oGeneration::H2O14 => "H2O14 (14th gen default)",
            H2oGeneration::H2O12 => "H2O12 (12th gen)",
        }
    }
}

/// Resolved path to `H2OOAE-Wx64.exe` for the selected generation.
pub fn h2ooae_exe(wrapper_root: &Path, generation: H2oGeneration) -> PathBuf {
    let sub = match generation {
        H2oGeneration::H2O14 => "H2O14",
        H2oGeneration::H2O12 => "H2O12",
    };
    wrapper_root.join(sub).join("H2OOAE-Wx64.exe")
}

/// Inject: `oa3tool /validate`, `H2OOAE-Wx64.exe -W <bin>`, `oa3tool /validate`.
pub fn inject_command_line(
    generation: H2oGeneration,
    wrapper_root: &Path,
    oa3_bin: &Path,
) -> String {
    let exe = h2ooae_exe(wrapper_root, generation);
    format!(
        "oa3tool /validate\n\"{}\" -W \"{}\"\noa3tool /validate\n",
        exe.display(),
        oa3_bin.display()
    )
}

/// Clear: `oa3tool /validate`, two `H2OOAE-Wx64.exe -E` lines (second with `NULL.BIN`), validate.
pub fn clear_command_line(generation: H2oGeneration, wrapper_root: &Path) -> String {
    let exe = h2ooae_exe(wrapper_root, generation);
    format!(
        "oa3tool /validate\n\"{}\" -E\n\"{}\" -E NULL.BIN\noa3tool /validate\n",
        exe.display(),
        exe.display()
    )
}

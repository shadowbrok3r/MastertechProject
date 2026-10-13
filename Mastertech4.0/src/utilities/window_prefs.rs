//! This machine's window-chrome preference and how the main window is framed this run.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::machine_settings;

/// How the main window is framed this run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chrome {
    /// MasterTech draws the title bar, caption buttons and resize edges.
    pub frameless: bool,
    /// The renderer can drop the OS decorations; false on the software renderer.
    pub available: bool,
}

#[derive(Serialize, Deserialize)]
struct WindowSettings {
    #[serde(default = "custom_title_bar_default")]
    custom_title_bar: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            custom_title_bar: custom_title_bar_default(),
        }
    }
}

fn custom_title_bar_default() -> bool {
    true
}

/// Per-machine settings file next to `update_settings.json`.
fn settings_file() -> PathBuf {
    machine_settings::dir().join("window_settings.json")
}

/// Whether this machine draws MasterTech's own title bar; on when the file is missing or unreadable.
pub fn load_custom_title_bar() -> bool {
    machine_settings::load::<WindowSettings>(&settings_file()).custom_title_bar
}

pub fn save_custom_title_bar(on: bool) -> anyhow::Result<()> {
    let path = settings_file();
    let mut settings: WindowSettings = machine_settings::load(&path);
    settings.custom_title_bar = on;
    machine_settings::save(&path, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_bar_choice_survives_a_round_trip_and_defaults_to_custom() {
        let dir =
            std::env::temp_dir().join(format!("mtech-window-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("window_settings.json");
        assert!(
            machine_settings::load::<WindowSettings>(&path).custom_title_bar,
            "a missing file means custom"
        );
        machine_settings::save(
            &path,
            &WindowSettings {
                custom_title_bar: false,
            },
        )
        .expect("save");
        assert!(!machine_settings::load::<WindowSettings>(&path).custom_title_bar);
        std::fs::write(&path, "{}").expect("write");
        assert!(
            machine_settings::load::<WindowSettings>(&path).custom_title_bar,
            "an absent field means custom"
        );
        std::fs::write(&path, "not json").expect("write");
        assert!(
            machine_settings::load::<WindowSettings>(&path).custom_title_bar,
            "an unreadable file means custom"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

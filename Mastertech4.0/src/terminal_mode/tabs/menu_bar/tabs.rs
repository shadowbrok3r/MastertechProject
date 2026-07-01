use strum::{EnumIter, IntoEnumIterator}; // Requires `strum` and `strum_macros` crates

////////////////////////////////////
/// TABS FOR MENU BAR
////////////////////////////////////
#[derive(Debug, Clone, Copy, Default, EnumIter, PartialEq, Eq, Hash)]
pub enum Tab {
    #[default]
    TurSheet,
    Scripts,
    Tasks,
    Ncdu,
    SystemInfo,
    Logs,
    Login,
    Webconsole,
    Settings,
    Assistant
}

impl Tab {
    #[allow(unused)]
    /// Get all tabs as a Vec for iteration
    fn all() -> Vec<Self> {
        Self::iter().collect()
    }

    #[allow(unused)]
    /// Get the next tab in the sequence, wrapping around to the start
    pub fn next(&self) -> Self {
        let tabs = Self::all();
        let current_idx = tabs.iter().position(|&t| t == *self).unwrap();
        let next_idx = (current_idx + 1) % tabs.len();
        tabs[next_idx]
    }

    #[allow(unused)]
    /// Get the previous tab in the sequence, wrapping around to the end
    pub fn prev(&self) -> Self {
        let tabs = Self::all();
        let current_idx = tabs.iter().position(|&t| t == *self).unwrap();
        let prev_idx = (current_idx + tabs.len() - 1) % tabs.len();
        tabs[prev_idx]
    }
}
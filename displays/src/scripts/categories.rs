//! Script category definitions with all available scripts

use super::{ScriptCategory, ScriptItem};
use std::collections::HashMap;

/// Returns all default tuneup scripts
pub fn tuneup_scripts() -> Vec<ScriptItem> {
    vec![
        ScriptItem::new("Data Transfer", ScriptCategory::Tuneup)
            .with_description("Transfer user data to a backup location"),
        ScriptItem::new("Activate CPS", ScriptCategory::Tuneup)
            .with_description("Install and activate Webroot and SuperAntiSpyware"),
        ScriptItem::new("Activate SEB", ScriptCategory::Tuneup)
            .with_description("Install and activate SuperEasyBackup"),
        ScriptItem::new("Install Windows Updates", ScriptCategory::Tuneup)
            .with_description("Check for and install Windows updates"),
        ScriptItem::new("Disable Sleep / Hibernation", ScriptCategory::Tuneup)
            .with_description("Disable sleep and hibernation power settings"),
        ScriptItem::new("Run SuperAntiSpyware Scan", ScriptCategory::Tuneup)
            .with_description("Run a full scan with SuperAntiSpyware"),
        ScriptItem::new("Run Webroot Scan", ScriptCategory::Tuneup)
            .with_description("Run a full scan with Webroot"),
        ScriptItem::new("Run Junkware Category", ScriptCategory::Tuneup)
            .with_description("Remove all known junkware applications"),
        ScriptItem::new("Run Tron", ScriptCategory::Tuneup)
            .with_description("Run Tron automated cleanup script"),
        ScriptItem::new("Install LibreOffice", ScriptCategory::Tuneup)
            .with_description("Install LibreOffice via Ninite"),
        ScriptItem::new("Disable proxy settings", ScriptCategory::Tuneup)
            .with_description("Disable any configured proxy settings"),
        ScriptItem::new("Disable Notifications", ScriptCategory::Tuneup)
            .with_description("Disable Windows notifications and suggestions"),
        ScriptItem::new("Change SuperAntiSpyware settings", ScriptCategory::Tuneup)
            .with_description("Configure SuperAntiSpyware scheduled tasks"),
        ScriptItem::new("Disable Startup Apps", ScriptCategory::Tuneup)
            .with_description("Disable unnecessary startup applications"),
        ScriptItem::new("Unpin Copilot", ScriptCategory::Tuneup)
            .with_description("Remove Copilot from taskbar"),
        ScriptItem::new("Align Taskbar to left", ScriptCategory::Tuneup)
            .with_description("Align Windows 11 taskbar to the left"),
        ScriptItem::new("Change Timezone to Mountain", ScriptCategory::Tuneup)
            .with_description("Set system timezone to Mountain Standard Time"),
        ScriptItem::new("Disable BitLocker", ScriptCategory::Tuneup)
            .with_description("Detect and disable BitLocker encryption on all drives"),
    ]
}

/// Returns all informational scripts
pub fn informational_scripts() -> Vec<ScriptItem> {
    vec![
        ScriptItem::new("Is SuperEasyBackup installed?", ScriptCategory::Informational)
            .with_pass_criteria("Installed and active")
            .with_warning_criteria("Not installed OR not active")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Is Webroot installed?", ScriptCategory::Informational)
            .with_pass_criteria("Installed and active")
            .with_warning_criteria("Not installed OR not active")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Is SuperAntiSpyware installed?", ScriptCategory::Informational)
            .with_pass_criteria("Installed and active")
            .with_warning_criteria("Not installed OR not active")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Are there scheduled tasks for it?", ScriptCategory::Informational)
            .with_description("Check if SuperAntiSpyware has scheduled tasks"),
        ScriptItem::new("Is Windows Activated?", ScriptCategory::Informational)
            .with_description("Check Windows activation status"),
        ScriptItem::new("Is Hibernation/Sleep enabled?", ScriptCategory::Informational)
            .with_description("Check power settings status"),
        ScriptItem::new("Any Recent Blue Screens?", ScriptCategory::Informational)
            .with_description("Check for recent BSOD events"),
        ScriptItem::new("When Was The Last Service Date?", ScriptCategory::Informational)
            .with_description("Query last service date from database"),
        ScriptItem::new("Windows Version", ScriptCategory::Informational)
            .with_pass_criteria("Windows 11")
            .with_warning_criteria("Windows 10")
            .with_error_criteria("Script Failed To Run"),
        ScriptItem::new("Check Updates", ScriptCategory::Informational)
            .with_description("Check for available Windows updates"),
        ScriptItem::new("Run Prechecks", ScriptCategory::Informational)
            .with_description("Run all informational prechecks"),
    ]
}

/// Returns all junkware removal scripts
pub fn junkware_scripts() -> Vec<ScriptItem> {
    vec![
        ScriptItem::new("OneLaunch", ScriptCategory::JunkwareRemoval)
            .with_description("Remove OneLaunch browser"),
        ScriptItem::new("WebNavigator Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove WebNavigator browser"),
        ScriptItem::new("Wave Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Wave Browser"),
        ScriptItem::new("Clear Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Clear Browser"),
        ScriptItem::new("Shift Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Shift Browser"),
        ScriptItem::new("Avast Browser", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Avast Browser"),
        ScriptItem::new("Mcaffee Safe", ScriptCategory::JunkwareRemoval)
            .with_description("Remove McAfee Safe Search"),
        ScriptItem::new("Driver Support", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Driver Support utility"),
        ScriptItem::new("Winzip", ScriptCategory::JunkwareRemoval)
            .with_description("Remove Winzip"),
        ScriptItem::new("Uninstall Microsoft 365", ScriptCategory::JunkwareRemoval)
            .with_description("Uninstall Microsoft 365 / Office apps"),
        ScriptItem::new("Uninstall OneDrive", ScriptCategory::JunkwareRemoval)
            .with_description("Uninstall Microsoft OneDrive"),
        ScriptItem::new("Disable OneDrive Startup", ScriptCategory::JunkwareRemoval)
            .with_description("Prevent OneDrive from launching at startup"),
        ScriptItem::new("Disable Edge Startup Boost", ScriptCategory::JunkwareRemoval)
            .with_description("Disable Microsoft Edge startup boost and background running"),
    ]
}

/// Get all default script categories with their scripts
pub fn get_all_categories() -> HashMap<ScriptCategory, Vec<ScriptItem>> {
    let mut categories = HashMap::new();
    categories.insert(ScriptCategory::Tuneup, tuneup_scripts());
    categories.insert(ScriptCategory::Informational, informational_scripts());
    categories.insert(ScriptCategory::JunkwareRemoval, junkware_scripts());
    categories
}

/// Category display order
pub const CATEGORY_ORDER: [ScriptCategory; 3] = [
    ScriptCategory::Tuneup,
    ScriptCategory::Informational,
    ScriptCategory::JunkwareRemoval,
];

/// Get category display name
pub fn category_display_name(category: &ScriptCategory) -> &'static str {
    match category {
        ScriptCategory::Tuneup => "Tuneup / QC",
        ScriptCategory::Informational => "Informational",
        ScriptCategory::JunkwareRemoval => "Junkware Removal",
        ScriptCategory::UserScripts(_) => "User Scripts",
        ScriptCategory::Custom(_) => "Custom",
    }
}

/// Get category icon (for egui)
pub fn category_icon(category: &ScriptCategory) -> &'static str {
    match category {
        ScriptCategory::Tuneup => "🔧",
        ScriptCategory::Informational => "ℹ",
        ScriptCategory::JunkwareRemoval => "🗑",
        ScriptCategory::UserScripts(_) => "📜",
        ScriptCategory::Custom(_) => "⚙️",
    }
}


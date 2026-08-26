//! The last-resort channel to an operator when no GUI and no console exist.
//!
//! Shown only when nothing else can carry the message: a launch with a console already has the
//! error on stderr, and an unattended relaunch must not block on a modal nobody will dismiss.

use std::path::Path;

/// Show a blocking native error dialog naming `log_path`. No-op when a console is attached.
pub fn gui_startup_failed(detail: &str, log_path: Option<&Path>) {
    if crate::console::has_console() {
        return;
    }
    let where_to_look = match log_path {
        Some(path) => format!("Log file:\n{}", path.display()),
        None => "No log file could be written on this machine.".to_owned(),
    };
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Mastertech could not start its interface")
        .set_description(format!("{detail}\n\n{where_to_look}"))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

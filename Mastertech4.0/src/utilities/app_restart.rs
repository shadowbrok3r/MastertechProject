/// Spawn a new Mastertech process in terminal (ratatui) mode and return
/// immediately.  The caller is responsible for closing the current window /
/// process afterwards if desired.
///
/// Equivalent to running `Mastertech.exe -t` from the command line.
pub fn restart_in_terminal_mode() -> std::io::Result<()> {
    let current_exe = std::env::current_exe()?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // The child draws a ratatui TUI, so it needs a console of its own regardless of this
        // process's subsystem. DETACHED_PROCESS gives it none; a `cmd /C` wrapper only worked
        // while this binary was console-subsystem, and left the wrapper's window on screen.
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        std::process::Command::new(&current_exe)
            .arg("-t")
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(&current_exe)
            .arg("-t")
            .spawn()?;
    }
    Ok(())
}

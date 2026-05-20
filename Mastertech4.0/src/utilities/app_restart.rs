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
        std::process::Command::new("cmd")
            .arg("/C")
            .arg(&current_exe)
            .arg("-t")
            .creation_flags(0x00000008) // DETACHED_PROCESS
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

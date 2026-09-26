//! Console-window suppression for child processes.

/// Process creation flag that keeps a console child from opening a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Starts a console child without a window on Windows; a no-op elsewhere.
pub trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(self, CREATE_NO_WINDOW);
        self
    }
}

impl NoWindow for tokio::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(CREATE_NO_WINDOW);
        self
    }
}

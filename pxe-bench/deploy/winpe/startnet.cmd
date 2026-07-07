@echo off
wpeinit
if not exist "%SystemRoot%\System32\MasterTech.exe" goto :nomt
rem --term = terminal (ratatui) mode, the PE path; --log attaches the parent
rem console so the TUI can render (this exe is windows-subsystem, no console otherwise).
"%SystemRoot%\System32\MasterTech.exe" --term --log
cmd /k
exit /b 0
:nomt
echo MasterTech.exe was not injected by wimboot - check the pxe-bench boot.ipxe initrd line.
cmd /k

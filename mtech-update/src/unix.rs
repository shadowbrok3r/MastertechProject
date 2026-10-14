//! Unix probe, swap and restart of a staged MasterTech binary.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};

use crate::{HostOs, check_kind, check_version, executable_mode, parse_version_banner, read_kind};

/// Spawn retries while another process still holds the staged file open for writing.
const BUSY_RETRIES: u32 = 20;
const BUSY_DELAY: Duration = Duration::from_millis(50);
/// Interval between child exit checks.
const POLL: Duration = Duration::from_millis(25);
/// Wait for the output pipes to drain after the child exits.
const PIPE_GRACE: Duration = Duration::from_secs(1);
/// Longest stderr excerpt carried in an error.
const EXCERPT_CHARS: usize = 400;

/// Running executable's path, without the ` (deleted)` suffix Linux appends once it is replaced.
pub fn running_exe() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(match exe.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
        Some(path) => PathBuf::from(OsStr::from_bytes(path)),
        None => exe,
    })
}

/// Gives `staged` the permission bits of `exe`, made executable.
pub fn make_executable(staged: &Path, exe: &Path) -> io::Result<()> {
    let mode = executable_mode(fs::metadata(exe)?.mode());
    fs::set_permissions(staged, fs::Permissions::from_mode(mode))
}

/// Best-effort chown of `staged` to the owner and group of `exe`.
pub fn match_owner(staged: &Path, exe: &Path) {
    let (Ok(staged_meta), Ok(exe_meta)) = (fs::metadata(staged), fs::metadata(exe)) else {
        return;
    };
    if (staged_meta.uid(), staged_meta.gid()) != (exe_meta.uid(), exe_meta.gid()) {
        let _ = std::os::unix::fs::chown(staged, Some(exe_meta.uid()), Some(exe_meta.gid()));
    }
}

/// Runs `staged --version` and returns the version it reports.
pub fn probe_version(staged: &Path, timeout: Duration) -> anyhow::Result<String> {
    let mut child = spawn_version(staged)?;
    let stdout = read_pipe(child.stdout.take());
    let stderr = read_pipe(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("update did not answer --version within {timeout:?}");
        }
        thread::sleep(POLL);
    };
    let stdout = stdout.recv_timeout(PIPE_GRACE).unwrap_or_default();
    let stderr = stderr.recv_timeout(PIPE_GRACE).unwrap_or_default();
    if !status.success() {
        let detail = excerpt(&stderr);
        if detail.is_empty() {
            bail!("update cannot start on this machine ({status})");
        }
        bail!("update cannot start on this machine ({status}): {detail}");
    }
    let stdout = String::from_utf8_lossy(&stdout);
    parse_version_banner(&stdout)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("update printed no version: {:?}", excerpt(stdout.as_bytes())))
}

fn spawn_version(staged: &Path) -> anyhow::Result<Child> {
    let mut retries = 0;
    loop {
        let spawned = Command::new(staged)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match spawned {
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy && retries < BUSY_RETRIES => {
                retries += 1;
                thread::sleep(BUSY_DELAY);
            }
            result => {
                return result.with_context(|| format!("could not run {}", staged.display()));
            }
        }
    }
}

/// Reads `pipe` to the end on its own thread.
fn read_pipe<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    rx
}

fn excerpt(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim()
        .chars()
        .take(EXCERPT_CHARS)
        .collect()
}

/// Checks `staged` against the running `exe`, makes it executable and confirms it reports `expected_version`.
pub fn prepare(
    staged: &Path,
    exe: &Path,
    expected_version: Option<&str>,
    timeout: Duration,
) -> anyhow::Result<()> {
    check_kind(read_kind(staged)?, HostOs::current(), read_kind(exe).ok())?;
    match_owner(staged, exe);
    make_executable(staged, exe)?;
    let reported = probe_version(staged, timeout)?;
    if let Some(expected) = expected_version {
        check_version(&reported, expected)?;
    }
    Ok(())
}

/// Validates `staged` and renames it over `exe`; `exe` is untouched on failure.
pub fn install(staged: &Path, exe: &Path, expected_len: u64) -> anyhow::Result<()> {
    let len = fs::metadata(staged)
        .map_err(|e| anyhow!("staged update missing at {staged:?}: {e}"))?
        .len();
    if expected_len > 0 && len != expected_len {
        bail!("staged update is {len} bytes, expected {expected_len}");
    }
    check_kind(read_kind(staged)?, HostOs::current(), read_kind(exe).ok())?;
    match_owner(staged, exe);
    make_executable(staged, exe)?;
    replace(staged, exe).map_err(|e| match e.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            let dir = exe.parent().unwrap_or(exe);
            anyhow!(
                "MasterTech cannot write {}: {e}. Move the binary to a folder you own.",
                dir.display()
            )
        }
        _ => anyhow!("could not install staged update: {e}"),
    })
}

/// Syncs `staged`, renames it over `exe` and syncs the directory.
pub fn replace(staged: &Path, exe: &Path) -> io::Result<()> {
    File::open(staged)?.sync_all()?;
    fs::rename(staged, exe)?;
    if let Some(dir) = exe.parent() {
        let _ = File::open(dir).and_then(|dir| dir.sync_all());
    }
    Ok(())
}

/// Replaces this process image with `exe`; returns only when exec fails.
pub fn exec_in_place(exe: &Path, args: &[OsString], envs: &[(&str, &str)]) -> io::Error {
    Command::new(exe)
        .args(args)
        .envs(envs.iter().copied())
        .exec()
}

/// Starts `exe` in a new process group with stdin detached.
pub fn spawn_detached(exe: &Path, args: &[OsString], envs: &[(&str, &str)]) -> io::Result<()> {
    Command::new(exe)
        .args(args)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::ffi::OsStringExt;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::*;

    const BANNER_SCRIPT: &str = "#!/bin/sh\necho 'Mastertech 9.9.9'\n";
    const EXEC_CHILD_ENV: &str = "MTECH_UPDATE_EXEC_CHILD";

    /// Serializes tests that write and then exec files.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_script(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().mode() & 0o7777
    }

    fn is_root() -> bool {
        let probe = tempfile::NamedTempFile::new().unwrap();
        probe.as_file().metadata().unwrap().uid() == 0
    }

    #[test]
    fn replace_swaps_a_running_binary_and_keeps_it_executable() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Mastertech-linux");
        fs::copy("/bin/sleep", &exe).unwrap();
        let mut running = Command::new(&exe).arg("5").spawn().unwrap();
        let staged = dir.path().join(".mastertech-update.staged");
        fs::write(&staged, BANNER_SCRIPT).unwrap();

        make_executable(&staged, &exe).unwrap();
        assert_eq!(probe_version(&staged, Duration::from_secs(10)).unwrap(), "9.9.9");
        replace(&staged, &exe).unwrap();

        assert!(!staged.exists());
        assert_eq!(mode(&exe) & 0o111, 0o111);
        let out = Command::new(&exe).arg("--version").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Mastertech 9.9.9");
        assert!(running.try_wait().unwrap().is_none());
        running.kill().unwrap();
        running.wait().unwrap();
    }

    #[test]
    fn probe_reports_a_binary_that_cannot_start() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        write_script(&staged, "#!/bin/sh\necho 'GLIBC_2.99 not found' >&2\nexit 1\n");

        let err = probe_version(&staged, Duration::from_secs(10)).unwrap_err();
        assert!(format!("{err:#}").contains("GLIBC_2.99"), "{err:#}");
    }

    #[test]
    fn probe_times_out() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        write_script(&staged, "#!/bin/sh\nexec sleep 60\n");

        let started = Instant::now();
        let err = probe_version(&staged, Duration::from_secs(1)).unwrap_err();
        assert!(err.to_string().contains("did not answer"), "{err:#}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn prepare_rejects_a_windows_payload_without_running_it() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Mastertech-linux");
        fs::copy("/bin/sleep", &exe).unwrap();
        let staged = dir.path().join(".mastertech-remote-update.staged");
        fs::write(&staged, b"MZ\x90\x00 not an ELF binary at all").unwrap();

        let err = prepare(&staged, &exe, None, Duration::from_secs(10)).unwrap_err();
        assert!(err.to_string().contains("Windows executable"), "{err:#}");
        assert_eq!(mode(&staged) & 0o111, 0);
    }

    #[test]
    fn make_executable_preserves_mode() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("exe");
        let staged = dir.path().join("staged");
        fs::write(&exe, b"exe").unwrap();
        fs::write(&staged, b"staged").unwrap();

        for (exe_mode, expected) in [(0o750, 0o750), (0o644, 0o755), (0o4755, 0o755)] {
            fs::set_permissions(&exe, fs::Permissions::from_mode(exe_mode)).unwrap();
            make_executable(&staged, &exe).unwrap();
            assert_eq!(mode(&staged), expected, "exe mode {exe_mode:o}");
        }
    }

    #[test]
    fn install_failures_leave_the_exe_untouched() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Mastertech-linux");
        fs::copy("/bin/sleep", &exe).unwrap();
        let original = fs::read(&exe).unwrap();
        let original_mode = mode(&exe);
        let staged = dir.path().join(".mastertech-update.staged");

        fs::copy("/bin/sleep", &staged).unwrap();
        let len = fs::metadata(&staged).unwrap().len();
        let err = install(&staged, &exe, len + 1).unwrap_err();
        assert!(err.to_string().contains("expected"), "{err:#}");

        fs::write(&staged, b"MZ\x90\x00 not an ELF binary at all").unwrap();
        let err = install(&staged, &exe, 0).unwrap_err();
        assert!(err.to_string().contains("Windows executable"), "{err:#}");

        if !is_root() {
            fs::copy("/bin/sleep", &staged).unwrap();
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
            let result = install(&staged, &exe, 0);
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
            let err = result.unwrap_err();
            assert!(err.to_string().contains("cannot write"), "{err:#}");
        }

        assert_eq!(fs::read(&exe).unwrap(), original);
        assert_eq!(mode(&exe), original_mode);
    }

    #[test]
    fn install_swaps_a_matching_binary() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Mastertech-linux");
        fs::copy("/bin/sleep", &exe).unwrap();
        let staged = dir.path().join(".mastertech-update.staged");
        fs::copy("/bin/sleep", &staged).unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600)).unwrap();
        let len = fs::metadata(&staged).unwrap().len();

        install(&staged, &exe, len).unwrap();
        assert!(!staged.exists());
        assert_eq!(mode(&exe) & 0o100, 0o100);
    }

    #[test]
    fn exec_keeps_the_pid() {
        if std::env::var_os(EXEC_CHILD_ENV).is_some() {
            let args = [OsString::from("-c"), OsString::from("echo pid=$$")];
            let err = exec_in_place(Path::new("/bin/sh"), &args, &[]);
            panic!("exec failed: {err}");
        }
        let _serial = serial();
        let module = module_path!().split_once("::").map_or("", |(_, rest)| rest);
        let name = format!("{module}::exec_keeps_the_pid");
        let child = Command::new(std::env::current_exe().unwrap())
            .args([name.as_str(), "--exact", "--nocapture", "--test-threads=1"])
            .env(EXEC_CHILD_ENV, "1")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let reported = stdout
            .lines()
            .find_map(|line| line.strip_prefix("pid="))
            .unwrap_or_else(|| panic!("no pid line in {stdout:?}"));
        assert_eq!(reported.trim(), pid.to_string());
    }

    #[test]
    fn relaunch_args_keep_non_utf8_args() {
        let raw = OsString::from_vec(vec![0xFF, b'x']);
        assert_eq!(
            crate::relaunch_args([raw.clone()], true),
            vec![raw, OsString::from("-t")]
        );
    }
}

//! MasterTech self-update core: release asset selection, executable checks and the Unix swap.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::bail;

#[cfg(unix)]
pub mod unix;

/// Lowercased file-name prefix of the MasterTech release asset.
const ASSET_PREFIX: &str = "mastertech";
/// Header bytes [`classify`] needs to identify an ELF binary.
pub const HEADER_LEN: usize = 20;

/// Operating system a MasterTech build runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    Linux,
    Other,
}

impl HostOs {
    /// OS this binary was compiled for.
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

/// Index of the MasterTech asset built for `os` among the release's asset `names`.
pub fn pick_update_asset<I, S>(names: I, os: HostOs) -> Option<usize>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    names
        .into_iter()
        .position(|name| is_update_asset(name.as_ref(), os))
}

fn is_update_asset(name: &str, os: HostOs) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with(ASSET_PREFIX)
        && match os {
            HostOs::Windows => name.ends_with(".exe"),
            HostOs::Linux => name.ends_with("-linux"),
            HostOs::Other => false,
        }
}

/// Executable format identified from a file header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExeKind {
    Pe,
    Elf { class64: bool, machine: u16 },
    Unknown,
}

impl ExeKind {
    fn describe(self) -> String {
        match self {
            Self::Pe => "Windows executable".to_string(),
            Self::Elf { class64, machine } => {
                let bits = if class64 { 64 } else { 32 };
                format!("{bits}-bit {} binary", machine_name(machine))
            }
            Self::Unknown => "unrecognized file".to_string(),
        }
    }
}

fn machine_name(machine: u16) -> String {
    match machine {
        0x03 => "x86".to_string(),
        0x28 => "ARM".to_string(),
        0x3E => "x86-64".to_string(),
        0xB7 => "AArch64".to_string(),
        0xF3 => "RISC-V".to_string(),
        other => format!("machine 0x{other:X}"),
    }
}

/// Identifies a PE or ELF executable from its first bytes.
pub fn classify(header: &[u8]) -> ExeKind {
    if header.starts_with(b"MZ") {
        return ExeKind::Pe;
    }
    if header.len() < HEADER_LEN || !header.starts_with(b"\x7fELF") {
        return ExeKind::Unknown;
    }
    let class64 = match header[4] {
        1 => false,
        2 => true,
        _ => return ExeKind::Unknown,
    };
    let raw = [header[18], header[19]];
    let machine = match header[5] {
        1 => u16::from_le_bytes(raw),
        2 => u16::from_be_bytes(raw),
        _ => return ExeKind::Unknown,
    };
    ExeKind::Elf { class64, machine }
}

/// Classifies the file at `path` by its header.
pub fn read_kind(path: &Path) -> std::io::Result<ExeKind> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    File::open(path)?
        .take(HEADER_LEN as u64)
        .read_to_end(&mut header)?;
    Ok(classify(&header))
}

/// Fails unless `staged` runs on `os` with the ELF class and machine of `running`.
pub fn check_kind(staged: ExeKind, os: HostOs, running: Option<ExeKind>) -> anyhow::Result<()> {
    match (os, staged) {
        (HostOs::Windows, ExeKind::Pe) => Ok(()),
        (HostOs::Windows, _) => bail!("update is not a Windows executable"),
        (HostOs::Linux, ExeKind::Pe) => {
            bail!("update is a Windows executable; this client runs Linux")
        }
        (HostOs::Linux, ExeKind::Unknown) => bail!("update is not a Linux executable"),
        (HostOs::Linux, ExeKind::Elf { .. }) => match running {
            Some(running @ ExeKind::Elf { .. }) if running != staged => bail!(
                "update is a {}; this client runs a {}",
                staged.describe(),
                running.describe()
            ),
            _ => Ok(()),
        },
        (HostOs::Other, _) => bail!("self-update is not supported on this OS"),
    }
}

/// Permission bits for a staged binary: `exe_mode` without special bits, executable wherever readable.
pub fn executable_mode(exe_mode: u32) -> u32 {
    let mode = exe_mode & 0o777;
    mode | ((mode & 0o444) >> 2) | 0o100
}

/// Forwarded CLI args, with `-t` appended when `terminal` and no arg already requests it.
pub fn relaunch_args<I>(args: I, terminal: bool) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().collect();
    if terminal && !args.iter().any(|arg| requests_terminal(arg)) {
        args.push(OsString::from("-t"));
    }
    args
}

/// `--term`, `-t`, or a short-flag cluster containing `t`.
fn requests_terminal(arg: &OsStr) -> bool {
    let Some(arg) = arg.to_str() else {
        return false;
    };
    arg == "--term"
        || arg.strip_prefix('-').is_some_and(|flags| {
            !flags.is_empty() && flags.chars().all(|c| c.is_ascii_alphabetic()) && flags.contains('t')
        })
}

/// Version from a MasterTech `--version` banner such as `Mastertech 5.0.2`.
pub fn parse_version_banner(output: &str) -> Option<&str> {
    output.lines().find_map(|line| {
        let mut tokens = line.split_whitespace();
        let (name, version) = (tokens.next()?, tokens.next()?);
        (name.eq_ignore_ascii_case("mastertech")
            && tokens.next().is_none()
            && version.starts_with(|c: char| c.is_ascii_digit()))
        .then_some(version)
    })
}

/// Fails unless `reported` equals the release tag `expected`, ignoring a leading `v`.
pub fn check_version(reported: &str, expected: &str) -> anyhow::Result<()> {
    let expected = expected.trim_start_matches('v');
    if reported != expected {
        bail!("update reports version {reported}, expected {expected}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_ASSETS: [&str; 3] = ["qc_app.exe", "Mastertech-linux", "Mastertech.exe"];

    fn elf(class: u8, data: u8, machine: [u8; 2]) -> Vec<u8> {
        let mut header = vec![0u8; 64];
        header[..4].copy_from_slice(b"\x7fELF");
        header[4] = class;
        header[5] = data;
        header[18..20].copy_from_slice(&machine);
        header
    }

    fn os_args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn linux_picks_the_linux_asset_and_windows_the_exe() {
        assert_eq!(pick_update_asset(RELEASE_ASSETS, HostOs::Linux), Some(1));
        assert_eq!(pick_update_asset(RELEASE_ASSETS, HostOs::Windows), Some(2));
        assert_eq!(pick_update_asset(RELEASE_ASSETS, HostOs::Other), None);
        assert_eq!(pick_update_asset(["qc_app.exe"], HostOs::Windows), None);
        assert_eq!(
            pick_update_asset(["qc_app.exe", "Mastertech-linux.sha256"], HostOs::Linux),
            None
        );
        assert_eq!(pick_update_asset(["MASTERTECH.EXE"], HostOs::Windows), Some(0));
        assert_eq!(pick_update_asset(["MasterTech-Linux"], HostOs::Linux), Some(0));
    }

    #[test]
    fn classify_reads_pe_and_elf_headers() {
        assert_eq!(classify(b"MZ\x90\x00"), ExeKind::Pe);
        assert_eq!(
            classify(&elf(2, 1, [0x3E, 0x00])),
            ExeKind::Elf { class64: true, machine: 0x3E }
        );
        assert_eq!(
            classify(&elf(1, 2, [0x00, 0x28])),
            ExeKind::Elf { class64: false, machine: 0x28 }
        );
        assert_eq!(classify(b"#!/bin/sh\necho 'Mastertech 9.9.9'\n"), ExeKind::Unknown);
        assert_eq!(classify(b"\x7fELF\x02\x01"), ExeKind::Unknown);
        assert_eq!(classify(&elf(3, 1, [0x3E, 0x00])), ExeKind::Unknown);
    }

    #[test]
    fn cross_platform_payloads_are_rejected() {
        let x64 = ExeKind::Elf { class64: true, machine: 0x3E };
        let arm64 = ExeKind::Elf { class64: true, machine: 0xB7 };

        let err = check_kind(ExeKind::Pe, HostOs::Linux, Some(x64)).unwrap_err();
        assert_eq!(err.to_string(), "update is a Windows executable; this client runs Linux");
        let err = check_kind(arm64, HostOs::Linux, Some(x64)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "update is a 64-bit AArch64 binary; this client runs a 64-bit x86-64 binary"
        );
        assert!(check_kind(ExeKind::Unknown, HostOs::Linux, Some(x64)).is_err());
        assert!(check_kind(x64, HostOs::Windows, None).is_err());
        assert!(check_kind(x64, HostOs::Other, Some(x64)).is_err());

        assert!(check_kind(x64, HostOs::Linux, Some(x64)).is_ok());
        assert!(check_kind(x64, HostOs::Linux, None).is_ok());
        assert!(check_kind(ExeKind::Pe, HostOs::Windows, None).is_ok());
    }

    #[test]
    fn terminal_relaunch_keeps_or_adds_dash_t() {
        assert_eq!(relaunch_args(os_args(&["-l"]), false), os_args(&["-l"]));
        assert_eq!(relaunch_args(os_args(&["-l"]), true), os_args(&["-l", "-t"]));
        assert_eq!(relaunch_args(os_args(&["-t"]), true), os_args(&["-t"]));
        assert_eq!(relaunch_args(os_args(&["--term"]), true), os_args(&["--term"]));
        assert_eq!(relaunch_args(os_args(&["-ct"]), true), os_args(&["-ct"]));
        assert_eq!(
            relaunch_args(os_args(&["--no-frost"]), true),
            os_args(&["--no-frost", "-t"])
        );
        assert_eq!(relaunch_args(Vec::new(), true), os_args(&["-t"]));
    }

    #[test]
    fn version_banner_parses() {
        assert_eq!(parse_version_banner("Mastertech 5.0.2\n"), Some("5.0.2"));
        assert_eq!(parse_version_banner("\nMastertech 5.0.2"), Some("5.0.2"));
        assert_eq!(parse_version_banner(""), None);
        assert_eq!(parse_version_banner("usage: Mastertech"), None);
        assert_eq!(parse_version_banner("ls (GNU coreutils) 9.4"), None);
        assert_eq!(parse_version_banner("other-tool 5.0.2"), None);
        assert_eq!(parse_version_banner("MasterTech 5.0.2 extra"), None);
        assert_eq!(parse_version_banner("MASTERTECH 5.0.2"), Some("5.0.2"));

        assert!(check_version("5.0.2", "5.0.2").is_ok());
        assert!(check_version("5.0.2", "v5.0.2").is_ok());
        assert_eq!(
            check_version("5.0.1", "5.0.2").unwrap_err().to_string(),
            "update reports version 5.0.1, expected 5.0.2"
        );
    }

    #[test]
    fn executable_mode_keeps_bits_and_adds_execute() {
        assert_eq!(executable_mode(0o100_750), 0o750);
        assert_eq!(executable_mode(0o644), 0o755);
        assert_eq!(executable_mode(0o4755), 0o755);
        assert_eq!(executable_mode(0o600), 0o700);
    }
}

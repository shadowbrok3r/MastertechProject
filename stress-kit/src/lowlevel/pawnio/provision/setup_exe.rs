//! Provisioning route that runs upstream's own installer.
//!
//! Carries every architecture and both editions, which is why it costs 3 MB of
//! binary where [`super::triplet`] costs 150 KB. It is the route upstream
//! supports, so it stays until the triplet route is proven on the bench.

use std::path::PathBuf;
use std::process::Command;

use crate::lowlevel::staging;

const SETUP_BYTES: &[u8] = include_bytes!("../../../../drivers/pawnio/PawnIO_setup.exe");
const SETUP_FILE: &str = "PawnIO_setup.exe";

/// Runs the embedded installer. `-install` selects the attestation-signed
/// edition; `-unrestricted` would select the unsigned one, which needs test
/// signing and is never what we want on a customer machine.
pub fn install() -> Result<(), String> {
    let path = setup_path()?;
    std::fs::write(&path, SETUP_BYTES)
        .map_err(|e| format!("could not stage the PawnIO installer at {}: {e}", path.display()))?;

    log::info!("stress-kit/pawnio: installing PawnIO from {}", path.display());
    let status = Command::new(&path).args(["-install", "-silent"]).status();
    let _ = std::fs::remove_file(&path);

    match status {
        Ok(code) if code.success() => Ok(()),
        Ok(code) => Err(format!(
            "the PawnIO installer exited with {code}; installing a driver package needs an \
             elevated process"
        )),
        Err(e) => Err(format!("could not run the PawnIO installer: {e}")),
    }
}

/// The installer runs elevated, so it is staged where only SYSTEM and
/// Administrators can write; a world-writable path would let another process
/// swap the image between the write and the launch.
fn setup_path() -> Result<PathBuf, String> {
    staging::driver_dir()
        .map(|dir| dir.join(SETUP_FILE))
        .ok_or_else(|| {
            "could not create the Mastertech drivers directory under %ProgramData% restricted \
             to SYSTEM and Administrators; refusing to run an installer from a world-writable \
             path"
                .to_string()
        })
}

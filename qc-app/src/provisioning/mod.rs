//! Company-as-data provisioning (QCWizard procedure port, non-destructive
//! P0–P1 slice): OS config (core isolation / timezone / open tools), DMI
//! writes (behind confirm), and G3 driver install from the `\\winbits7` share.
//! Destructive steps (diskpart, cleanup) and the full G4 software matrix are
//! deferred to a later phase.

pub mod catalog_query;
pub mod cleanup;
pub mod company;
pub mod dmi;
pub mod download;
pub mod drivers;
pub mod manifest;
pub mod osconfig;
pub mod procedure;
pub mod software;
pub mod vendor_steps;

pub use company::Company;
pub use manifest::{load as load_manifest, CompanyManifest};

use anyhow::anyhow;

/// Detect the board, look up its chipset driver, and install it.
pub fn install_chipset(sqlite_path: &str) -> anyhow::Result<String> {
    let product = crate::hardware_id::read_baseboard_product()
        .ok_or_else(|| anyhow!("no motherboard product detected"))?;
    let conn = rusqlite::Connection::open(sqlite_path)?;
    let row = catalog_query::chipset_driver_for_baseboard(&conn, &product)?
        .ok_or_else(|| anyhow!("no chipset driver mapped for board '{product}'"))?;
    drivers::install_driver(&row)
}

/// Detect GPU device code(s), look up the display driver (NVIDIA desktop id 1
/// fallback), and install it.
pub fn install_display(sqlite_path: &str) -> anyhow::Result<String> {
    let conn = rusqlite::Connection::open(sqlite_path)?;
    let codes = crate::hardware_id::read_gpu_device_codes();
    for code in &codes {
        if let Some(row) = catalog_query::gpu_driver_for_device(&conn, code)? {
            return drivers::install_driver(&row);
        }
    }
    if let Some(row) = catalog_query::driver_by_id(&conn, 1)? {
        return drivers::install_driver(&row);
    }
    Err(anyhow!("no display driver mapped for detected GPU(s): {codes:?}"))
}

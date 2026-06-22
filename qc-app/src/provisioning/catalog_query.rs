//! Driver-catalog lookups over the Swift SQLite DB (`schema.rs`). Turns the
//! lookup-only catalog into the G3 install source: baseboard → chipset driver,
//! GPU device code → display driver, plus id fallbacks.

use rusqlite::{Connection, OptionalExtension};

/// A driver row resolved for install.
#[derive(Debug, Clone, PartialEq)]
pub struct DriverRow {
    pub id: i64,
    pub file_name: String,
    pub url_download: Option<String>,
    pub argument_string: Option<String>,
    pub id_file_type: i64,
    pub version: Option<String>,
}

const SELECT: &str =
    "SELECT id, file_name, url_download, id_file_type, argument_string, version FROM driver";

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<DriverRow> {
    Ok(DriverRow {
        id: row.get(0)?,
        file_name: row.get(1)?,
        url_download: row.get(2)?,
        id_file_type: row.get(3)?,
        argument_string: row.get(4)?,
        version: row.get(5)?,
    })
}

/// Chipset driver for a motherboard product: baseboard → package.id_chipset_driver → driver.
pub fn chipset_driver_for_baseboard(conn: &Connection, product: &str) -> rusqlite::Result<Option<DriverRow>> {
    let sql = format!(
        "{SELECT} WHERE id = (SELECT p.id_chipset_driver FROM baseboard b \
         JOIN package p ON p.id = b.id_package WHERE b.product = ?1)"
    );
    conn.query_row(&sql, [product], map_row).optional()
}

/// Display driver for a GPU device code: device.code → graphics_card.id_driver → driver.
pub fn gpu_driver_for_device(conn: &Connection, device_code: &str) -> rusqlite::Result<Option<DriverRow>> {
    let sql = format!(
        "{SELECT} WHERE id = (SELECT g.id_driver FROM graphics_card g \
         JOIN device d ON d.id = g.id_device WHERE d.code = ?1)"
    );
    conn.query_row(&sql, [device_code], map_row).optional()
}

/// Direct id lookup — NVIDIA desktop (1) / studio (45/46) fallbacks.
pub fn driver_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<DriverRow>> {
    let sql = format!("{SELECT} WHERE id = ?1");
    conn.query_row(&sql, [id], map_row).optional()
}

/// Latest BIOS file + release page for a motherboard product.
#[derive(Debug, Clone)]
pub struct BiosInfo {
    pub file_name: Option<String>,
    pub url_webpage: String,
}

/// BIOS row for a motherboard product: baseboard.id_bios → bios.
pub fn bios_info_for_baseboard(conn: &Connection, product: &str) -> rusqlite::Result<Option<BiosInfo>> {
    let sql = "SELECT b.file_name, b.url_webpage FROM bios b \
               JOIN baseboard bb ON bb.id_bios = b.id WHERE bb.product = ?1";
    conn.query_row(sql, [product], |row| {
        Ok(BiosInfo { file_name: row.get(0)?, url_webpage: row.get(1)? })
    })
    .optional()
}

/// A catalog target driver: install file + optional version string.
#[derive(Debug, Clone, Default)]
pub struct TargetDriver {
    pub file: String,
    pub version: Option<String>,
}

/// Catalog target driver per package category for a board.
#[derive(Debug, Clone, Default)]
pub struct PackageDrivers {
    pub chipset: Option<TargetDriver>,
    pub me: Option<TargetDriver>,
    pub graphics: Option<TargetDriver>,
    pub audio: Option<TargetDriver>,
    pub lan: Option<TargetDriver>,
    pub bluetooth: Option<TargetDriver>,
    pub wifi: Option<TargetDriver>,
    pub raid: Option<TargetDriver>,
    pub control_center: Option<TargetDriver>,
}

/// Every catalog target driver mapped to a board's package, by category.
pub fn package_drivers_for_baseboard(
    conn: &Connection,
    product: &str,
) -> rusqlite::Result<Option<PackageDrivers>> {
    let sql = "SELECT cd.file_name, cd.version, me.file_name, me.version, gd.file_name, gd.version, \
               ad.file_name, ad.version, ld.file_name, ld.version, bt.file_name, bt.version, \
               wf.file_name, wf.version, rd.file_name, rd.version, cc.file_name, cc.version \
               FROM baseboard b JOIN package p ON p.id = b.id_package \
               LEFT JOIN driver cd ON cd.id = p.id_chipset_driver \
               LEFT JOIN driver me ON me.id = p.id_me_driver \
               LEFT JOIN driver gd ON gd.id = p.id_graphics_driver \
               LEFT JOIN driver ad ON ad.id = p.id_audio_driver \
               LEFT JOIN driver ld ON ld.id = p.id_lan_driver \
               LEFT JOIN driver bt ON bt.id = p.id_bluetooth_driver \
               LEFT JOIN driver wf ON wf.id = p.id_wifi_driver \
               LEFT JOIN driver rd ON rd.id = p.id_raid_driver \
               LEFT JOIN driver cc ON cc.id = p.id_control_center_driver \
               WHERE b.product = ?1";
    conn.query_row(sql, [product], |row| {
        let mk = |file: Option<String>, version: Option<String>| {
            file.map(|f| TargetDriver { file: f, version })
        };
        Ok(PackageDrivers {
            chipset: mk(row.get(0)?, row.get(1)?),
            me: mk(row.get(2)?, row.get(3)?),
            graphics: mk(row.get(4)?, row.get(5)?),
            audio: mk(row.get(6)?, row.get(7)?),
            lan: mk(row.get(8)?, row.get(9)?),
            bluetooth: mk(row.get(10)?, row.get(11)?),
            wifi: mk(row.get(12)?, row.get(13)?),
            raid: mk(row.get(14)?, row.get(15)?),
            control_center: mk(row.get(16)?, row.get(17)?),
        })
    })
    .optional()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::schema::SQLITE_SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO file_type (id, name) VALUES (1, 'exe');
             INSERT INTO driver (id, file_name, url_download, id_file_type, argument_string)
                 VALUES (10, 'intel_chipset.exe', 'Intel/intel_chipset.exe', 1, '-s -overwrite');
             INSERT INTO driver (id, file_name, id_file_type) VALUES (1, 'nvidia_desktop.exe', 1);
             INSERT INTO driver (id, file_name, id_file_type) VALUES (20, 'rtx_display.exe', 1);
             INSERT INTO vendor (id, name, code) VALUES (1, 'NVIDIA', '10DE');
             INSERT INTO device (id, name, code) VALUES (1, 'RTX 5090', '2C02');
             INSERT INTO graphics_card (id, id_vendor, id_device, id_driver) VALUES (1, 1, 1, 20);
             INSERT INTO manufacturer (id, name) VALUES (1, 'MSI');
             INSERT INTO bios (id, url_webpage, url_download) VALUES (1, 'http://x', 'http://x/dl');
             INSERT INTO package (id, id_chipset_driver, id_audio_driver, id_lan_driver, id_raid_driver)
                 VALUES (1, 10, 10, 10, 10);
             INSERT INTO baseboard (id, product, id_manufacturer, id_package, id_bios)
                 VALUES (1, 'MSI MEG X670E GODLIKE', 1, 1, 1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn chipset_by_baseboard() {
        let conn = seed();
        let d = chipset_driver_for_baseboard(&conn, "MSI MEG X670E GODLIKE").unwrap().unwrap();
        assert_eq!(d.file_name, "intel_chipset.exe");
        assert_eq!(d.url_download.as_deref(), Some("Intel/intel_chipset.exe"));
        assert_eq!(d.argument_string.as_deref(), Some("-s -overwrite"));
        assert!(chipset_driver_for_baseboard(&conn, "UNKNOWN BOARD").unwrap().is_none());
    }

    #[test]
    fn gpu_by_device_code() {
        let conn = seed();
        let d = gpu_driver_for_device(&conn, "2C02").unwrap().unwrap();
        assert_eq!(d.file_name, "rtx_display.exe");
        assert!(gpu_driver_for_device(&conn, "FFFF").unwrap().is_none());
    }

    #[test]
    fn id_fallback() {
        let conn = seed();
        assert_eq!(driver_by_id(&conn, 1).unwrap().unwrap().file_name, "nvidia_desktop.exe");
        assert!(driver_by_id(&conn, 999).unwrap().is_none());
    }

    #[test]
    fn package_drivers_maps_categories() {
        let conn = seed();
        let p = package_drivers_for_baseboard(&conn, "MSI MEG X670E GODLIKE").unwrap().unwrap();
        assert_eq!(p.chipset.as_ref().map(|t| t.file.as_str()), Some("intel_chipset.exe"));
        assert_eq!(p.audio.as_ref().map(|t| t.file.as_str()), Some("intel_chipset.exe"));
        assert_eq!(p.lan.as_ref().map(|t| t.file.as_str()), Some("intel_chipset.exe"));
        assert_eq!(p.raid.as_ref().map(|t| t.file.as_str()), Some("intel_chipset.exe"));
        assert!(p.wifi.is_none());
        assert!(p.bluetooth.is_none());
        assert!(package_drivers_for_baseboard(&conn, "UNKNOWN").unwrap().is_none());
    }
}

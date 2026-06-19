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
}

const SELECT: &str = "SELECT id, file_name, url_download, id_file_type, argument_string FROM driver";

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<DriverRow> {
    Ok(DriverRow {
        id: row.get(0)?,
        file_name: row.get(1)?,
        url_download: row.get(2)?,
        id_file_type: row.get(3)?,
        argument_string: row.get(4)?,
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
}

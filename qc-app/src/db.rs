use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::Connection;

use crate::schema::SQLITE_SCHEMA;

/// Local SQLite file for Swift driver metadata (see `schema.rs`).
pub fn default_sqlite_path() -> PathBuf {
    match directories::ProjectDirs::from("com", "Mastertech", "MastertechQC") {
        Some(p) => p.data_local_dir().join("swift_driver.sqlite"),
        None => std::env::temp_dir().join("mastertech_qc_swift_driver.sqlite"),
    }
}

/// Open (or create) the Swift driver SQLite DB and apply schema migrations.
pub fn open_or_create(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    let conn = Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
    conn.execute_batch(SQLITE_SCHEMA)
        .context("apply qc_app schema")?;
    Ok(conn)
}

/// `(table_name, row_count)` for known driver tables.
pub fn table_stats(conn: &Connection) -> anyhow::Result<Vec<(String, i64)>> {
    let tables = [
        "file_type",
        "driver_type",
        "driver",
        "vendor",
        "device",
        "bios",
        "manufacturer",
        "graphics_card",
        "package",
        "baseboard",
    ];
    let mut out = Vec::new();
    for t in tables {
        let n: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {t}"),
            [],
            |row| row.get(0),
        )?;
        out.push((t.to_string(), n));
    }
    Ok(out)
}

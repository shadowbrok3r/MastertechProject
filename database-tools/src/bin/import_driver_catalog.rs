//! Import the Swift driver catalog from qc-app's local SQLite file into SurrealDB.
//!
//! qc-app resolves a board's expected drivers and BIOS from
//! `swift_driver.sqlite`, a file that only exists on a QC bench machine. This
//! lifts that catalog into the `catalog_*` tables so every admin console
//! resolves the same targets.
//!
//! Ids are deterministic (`database::schema::driver_catalog::*_record_id`), so
//! re-running against a refreshed SQLite file upserts in place rather than
//! duplicating. Rows whose natural key is blank are skipped: a blank value in a
//! UNIQUE index silently orphans every later blank write.
//!
//! ```text
//! cargo run -p database-tools --bin import-driver-catalog -- --dry-run
//! cargo run --release -p database-tools --bin import-driver-catalog -- --sqlite C:\path\to\swift_driver.sqlite
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use database::db;
use database::schema::driver_catalog::{
    baseboard_record_id, bios_record_id, driver_record_id, gpu_device_record_id,
    normalize_device_code, package_record_id, CatalogBaseboard, CatalogBios, CatalogDriver,
    CatalogGpuDevice, CatalogPackage,
};
use database::schema::{RecordId, RecordIdExt};
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Parser)]
#[command(version, about = "Import qc-app's Swift driver catalog (SQLite) into SurrealDB.")]
struct Args {
    /// Catalog file. Defaults to qc-app's own path for this user.
    #[arg(long)]
    sqlite: Option<PathBuf>,

    /// Read and report, write nothing.
    #[arg(long)]
    dry_run: bool,
}

/// qc-app's `db::default_sqlite_path`, duplicated so this tool does not have to
/// depend on the qc-app crate.
fn default_sqlite_path() -> PathBuf {
    match directories::ProjectDirs::from("com", "Mastertech", "MastertechQC") {
        Some(p) => p.data_local_dir().join("swift_driver.sqlite"),
        None => std::env::temp_dir().join("mastertech_qc_swift_driver.sqlite"),
    }
}

#[derive(Default)]
struct Counts {
    drivers: usize,
    bios: usize,
    packages: usize,
    gpu_devices: usize,
    baseboards: usize,
    skipped: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .try_init()
        .ok();

    let args = Args::parse();
    let path = args.sqlite.unwrap_or_else(default_sqlite_path);
    let conn = Connection::open(&path)
        .with_context(|| format!("open sqlite catalog {}", path.display()))?;
    for table in ["driver", "file_type", "bios", "package", "baseboard", "graphics_card", "device"] {
        anyhow::ensure!(
            table_is_present(&conn, table)?,
            "{} has no `{table}` table — is this the Swift driver catalog?",
            path.display()
        );
    }
    log::info!("reading catalog {}", path.display());

    if !args.dry_run {
        database::init_database()
            .await
            .context("could not connect to database — check .env and that surreal is running")?;
        log::info!("connected to ns={} db={}", database::NS, database::DB);
    }

    let mut counts = Counts::default();

    // --- drivers: source id -> record id, needed by every later table ---
    let file_types = read_file_types(&conn)?;
    let mut driver_ids: HashMap<i64, RecordId> = HashMap::new();
    // Two file names can slug to the same key; the second write would replace
    // the first in place and silently re-point every package that linked it.
    let mut claimed: HashMap<String, String> = HashMap::new();
    for row in read_drivers(&conn)? {
        let Some(rid) = driver_record_id(&row.file_name) else {
            counts.skipped.push(format!("driver {} has a blank file_name", row.id));
            continue;
        };
        let key = rid.key_string();
        if let Some(first) = claimed.get(&key) {
            counts.skipped.push(format!(
                "driver {} '{}' collides with '{first}' on catalog id '{key}'",
                row.id, row.file_name
            ));
            continue;
        }
        claimed.insert(key, row.file_name.clone());
        driver_ids.insert(row.id, rid.clone());
        let entry = CatalogDriver {
            id: rid.clone(),
            file_name: row.file_name,
            url_download: row.url_download,
            argument_string: row.argument_string,
            file_type: file_types.get(&row.id_file_type).cloned().unwrap_or_default(),
            version: row.version,
            source_id: Some(row.id),
            imported_at: chrono::Utc::now().into(),
        };
        if !args.dry_run {
            let _: Option<CatalogDriver> = db().upsert(rid).content(entry).await?;
        }
        counts.drivers += 1;
    }

    // --- bios ---
    let mut bios_ids: HashMap<i64, RecordId> = HashMap::new();
    for row in read_bios(&conn)? {
        let rid = bios_record_id(row.id);
        bios_ids.insert(row.id, rid.clone());
        let entry = CatalogBios {
            id: rid.clone(),
            file_name: row.file_name,
            url_webpage: row.url_webpage,
            url_download: row.url_download,
            source_id: Some(row.id),
            imported_at: chrono::Utc::now().into(),
        };
        if !args.dry_run {
            let _: Option<CatalogBios> = db().upsert(rid).content(entry).await?;
        }
        counts.bios += 1;
    }

    // --- packages ---
    let mut package_ids: HashMap<i64, RecordId> = HashMap::new();
    for row in read_packages(&conn)? {
        let rid = package_record_id(row.id);
        package_ids.insert(row.id, rid.clone());
        let link = |id: Option<i64>| id.and_then(|i| driver_ids.get(&i).cloned());
        let entry = CatalogPackage {
            id: rid.clone(),
            chipset: link(row.chipset),
            me: link(row.me),
            graphics: link(row.graphics),
            audio: link(row.audio),
            lan: link(row.lan),
            bluetooth: link(row.bluetooth),
            wifi: link(row.wifi),
            raid: link(row.raid),
            control_center: link(row.control_center),
            source_id: Some(row.id),
            imported_at: chrono::Utc::now().into(),
        };
        if !args.dry_run {
            let _: Option<CatalogPackage> = db().upsert(rid).content(entry).await?;
        }
        counts.packages += 1;
    }

    // --- gpu devices ---
    for row in read_gpu_devices(&conn)? {
        let code = normalize_device_code(&row.code);
        let Some(rid) = gpu_device_record_id(&code) else {
            counts.skipped.push(format!("graphics_card {} has a blank device code", row.id));
            continue;
        };
        let entry = CatalogGpuDevice {
            id: rid.clone(),
            code,
            device_name: row.device_name,
            vendor: row.vendor,
            driver: row.id_driver.and_then(|i| driver_ids.get(&i).cloned()),
            source_id: Some(row.id),
            imported_at: chrono::Utc::now().into(),
        };
        if !args.dry_run {
            let _: Option<CatalogGpuDevice> = db().upsert(rid).content(entry).await?;
        }
        counts.gpu_devices += 1;
    }

    // --- baseboards ---
    for row in read_baseboards(&conn)? {
        let Some(rid) = baseboard_record_id(&row.product) else {
            counts.skipped.push(format!("baseboard {} has a blank product", row.id));
            continue;
        };
        let entry = CatalogBaseboard {
            id: rid.clone(),
            product: row.product,
            manufacturer: row.manufacturer,
            package: package_ids.get(&row.id_package).cloned(),
            bios: bios_ids.get(&row.id_bios).cloned(),
            source_id: Some(row.id),
            imported_at: chrono::Utc::now().into(),
        };
        if !args.dry_run {
            let _: Option<CatalogBaseboard> = db().upsert(rid).content(entry).await?;
        }
        counts.baseboards += 1;
    }

    let verb = if args.dry_run { "would import" } else { "imported" };
    println!(
        "{verb}: {} drivers, {} bios, {} packages, {} gpu devices, {} baseboards",
        counts.drivers, counts.bios, counts.packages, counts.gpu_devices, counts.baseboards
    );
    for note in &counts.skipped {
        println!("skipped: {note}");
    }
    Ok(())
}

// ============================================================
// SQLite readers — shapes mirror qc-app's `schema::SQLITE_SCHEMA`
// ============================================================

struct DriverRow {
    id: i64,
    file_name: String,
    url_download: Option<String>,
    argument_string: Option<String>,
    id_file_type: i64,
    version: Option<String>,
}

struct BiosRow {
    id: i64,
    file_name: Option<String>,
    url_webpage: String,
    url_download: Option<String>,
}

struct PackageRow {
    id: i64,
    chipset: Option<i64>,
    me: Option<i64>,
    graphics: Option<i64>,
    audio: Option<i64>,
    lan: Option<i64>,
    bluetooth: Option<i64>,
    wifi: Option<i64>,
    raid: Option<i64>,
    control_center: Option<i64>,
}

struct GpuDeviceRow {
    id: i64,
    code: String,
    device_name: String,
    vendor: String,
    id_driver: Option<i64>,
}

struct BaseboardRow {
    id: i64,
    product: String,
    manufacturer: String,
    id_package: i64,
    id_bios: i64,
}

fn read_file_types(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT id, name FROM file_type")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

fn read_drivers(conn: &Connection) -> Result<Vec<DriverRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_name, url_download, argument_string, id_file_type, version FROM driver",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DriverRow {
            id: r.get(0)?,
            file_name: r.get(1)?,
            url_download: r.get(2)?,
            argument_string: r.get(3)?,
            id_file_type: r.get(4)?,
            version: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn read_bios(conn: &Connection) -> Result<Vec<BiosRow>> {
    let mut stmt = conn.prepare("SELECT id, file_name, url_webpage, url_download FROM bios")?;
    let rows = stmt.query_map([], |r| {
        Ok(BiosRow {
            id: r.get(0)?,
            file_name: r.get(1)?,
            url_webpage: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            url_download: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn read_packages(conn: &Connection) -> Result<Vec<PackageRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, id_chipset_driver, id_me_driver, id_graphics_driver, id_audio_driver, \
         id_lan_driver, id_bluetooth_driver, id_wifi_driver, id_raid_driver, \
         id_control_center_driver FROM package",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(PackageRow {
            id: r.get(0)?,
            chipset: r.get(1)?,
            me: r.get(2)?,
            graphics: r.get(3)?,
            audio: r.get(4)?,
            lan: r.get(5)?,
            bluetooth: r.get(6)?,
            wifi: r.get(7)?,
            raid: r.get(8)?,
            control_center: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// `graphics_card` joined out to the device code and vendor name qc-app looks
/// the display driver up by.
fn read_gpu_devices(conn: &Connection) -> Result<Vec<GpuDeviceRow>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, d.code, d.name, v.name, g.id_driver FROM graphics_card g \
         JOIN device d ON d.id = g.id_device \
         LEFT JOIN vendor v ON v.id = g.id_vendor",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(GpuDeviceRow {
            id: r.get(0)?,
            code: r.get(1)?,
            device_name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            vendor: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            id_driver: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn read_baseboards(conn: &Connection) -> Result<Vec<BaseboardRow>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.product, m.name, b.id_package, b.id_bios FROM baseboard b \
         LEFT JOIN manufacturer m ON m.id = b.id_manufacturer",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(BaseboardRow {
            id: r.get(0)?,
            product: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            manufacturer: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            id_package: r.get(3)?,
            id_bios: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Guards against pointing the tool at an unrelated SQLite file.
fn table_is_present(conn: &Connection, table: &str) -> Result<bool> {
    let found: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

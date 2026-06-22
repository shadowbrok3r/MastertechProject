//! SQLite DDL ported from QC-Project `db_creation_script.sql` (MySQL → `IF NOT EXISTS`).

/// Applied on open via `db::open_or_create`.
pub const SQLITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS file_type (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS driver_type (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS driver (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_name TEXT NOT NULL UNIQUE,
    url_download TEXT,
    id_file_type INTEGER NOT NULL DEFAULT 1,
    argument_string TEXT,
    version TEXT,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (id_file_type) REFERENCES file_type(id)
);

CREATE TABLE IF NOT EXISTS vendor (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    code TEXT NOT NULL UNIQUE,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS device (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    code TEXT NOT NULL UNIQUE,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS bios (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url_webpage TEXT NOT NULL UNIQUE,
    url_download TEXT NOT NULL UNIQUE,
    file_name TEXT,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS manufacturer (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS graphics_card (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    id_vendor INTEGER NOT NULL,
    id_device INTEGER NOT NULL,
    id_driver INTEGER,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (id_vendor) REFERENCES vendor(id),
    FOREIGN KEY (id_device) REFERENCES device(id),
    FOREIGN KEY (id_driver) REFERENCES driver(id)
);

CREATE TABLE IF NOT EXISTS package (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    id_chipset_driver INTEGER NOT NULL,
    id_me_driver INTEGER,
    id_graphics_driver INTEGER,
    id_audio_driver INTEGER NOT NULL,
    id_lan_driver INTEGER NOT NULL,
    id_bluetooth_driver INTEGER,
    id_wifi_driver INTEGER,
    id_raid_driver INTEGER NOT NULL,
    id_control_center_driver INTEGER,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (id_chipset_driver) REFERENCES driver(id),
    FOREIGN KEY (id_me_driver) REFERENCES driver(id),
    FOREIGN KEY (id_graphics_driver) REFERENCES driver(id),
    FOREIGN KEY (id_audio_driver) REFERENCES driver(id),
    FOREIGN KEY (id_lan_driver) REFERENCES driver(id),
    FOREIGN KEY (id_bluetooth_driver) REFERENCES driver(id),
    FOREIGN KEY (id_wifi_driver) REFERENCES driver(id),
    FOREIGN KEY (id_raid_driver) REFERENCES driver(id),
    FOREIGN KEY (id_control_center_driver) REFERENCES driver(id)
);

CREATE TABLE IF NOT EXISTS baseboard (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    product TEXT UNIQUE,
    id_manufacturer INTEGER NOT NULL,
    id_package INTEGER NOT NULL,
    id_bios INTEGER NOT NULL,
    date_created TEXT NOT NULL DEFAULT (datetime('now')),
    last_upd TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (id_manufacturer) REFERENCES manufacturer(id),
    FOREIGN KEY (id_package) REFERENCES package(id),
    FOREIGN KEY (id_bios) REFERENCES bios(id)
);
"#;

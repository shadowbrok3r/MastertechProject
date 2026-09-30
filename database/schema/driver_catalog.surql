-- ============================================================
-- TABLES: catalog_driver / catalog_bios / catalog_package /
--         catalog_gpu_device / catalog_baseboard
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Fleet driver catalog: the canonical driver/BIOS a board or GPU is
-- supposed to be running. Ported from qc-app's local `swift_driver.sqlite`
-- so every admin console resolves the same targets instead of depending on
-- a file that only exists on a QC bench machine.
--
-- Lookup shape mirrors the SQLite one: SMBIOS baseboard product ->
-- catalog_baseboard -> catalog_package (one driver per category) and
-- catalog_bios; PCI device code -> catalog_gpu_device -> catalog_driver.
-- Rust source of truth: `database::schema::driver_catalog`.
--
-- Indexes are defined after the fields they cover: a file is applied in
-- order and an index on a not-yet-defined field aborts the sync.
--
-- Rows are keyed deterministically (`driver_catalog::*_record_id`) so a
-- re-import upserts in place. The importer drops rows whose natural key is
-- blank — one `''` in a UNIQUE index silently orphans every later blank write.

-- One installable driver file.
DEFINE TABLE catalog_driver TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD file_name          ON catalog_driver TYPE string                          PERMISSIONS FULL;
DEFINE FIELD url_download       ON catalog_driver TYPE none | string                   PERMISSIONS FULL;
-- Silent-install switches the provisioner passes to the installer.
DEFINE FIELD argument_string    ON catalog_driver TYPE none | string                   PERMISSIONS FULL;
-- Catalog `file_type.name` ('exe', 'msi', 'zip', …).
DEFINE FIELD file_type          ON catalog_driver TYPE string DEFAULT ''               PERMISSIONS FULL;
DEFINE FIELD version            ON catalog_driver TYPE none | string                   PERMISSIONS FULL;
-- Row id in the SQLite catalog this was imported from.
DEFINE FIELD source_id          ON catalog_driver TYPE none | int                      PERMISSIONS FULL;
DEFINE FIELD imported_at        ON catalog_driver TYPE datetime DEFAULT time::now()    PERMISSIONS FULL;

DEFINE INDEX cdrv_file_name     ON catalog_driver FIELDS file_name UNIQUE;

-- Latest BIOS file + release page for a board.
DEFINE TABLE catalog_bios TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD file_name          ON catalog_bios TYPE none | string                     PERMISSIONS FULL;
DEFINE FIELD url_webpage        ON catalog_bios TYPE string DEFAULT ''                 PERMISSIONS FULL;
DEFINE FIELD url_download       ON catalog_bios TYPE none | string                     PERMISSIONS FULL;
DEFINE FIELD source_id          ON catalog_bios TYPE none | int                        PERMISSIONS FULL;
DEFINE FIELD imported_at        ON catalog_bios TYPE datetime DEFAULT time::now()      PERMISSIONS FULL;

-- The per-board driver set, one link per part category. Every category is
-- optional: a board with no discrete RAID controller has no raid driver.
DEFINE TABLE catalog_package TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD chipset            ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD me                 ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD graphics           ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD audio              ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD lan                ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD bluetooth          ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD wifi               ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD raid               ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD control_center     ON catalog_package TYPE none | record<catalog_driver>  PERMISSIONS FULL;
DEFINE FIELD source_id          ON catalog_package TYPE none | int                     PERMISSIONS FULL;
DEFINE FIELD imported_at        ON catalog_package TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;

-- PCI device code -> display driver. The GPU target comes from the card, not
-- the board, so it is looked up separately from the package.
DEFINE TABLE catalog_gpu_device TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- PCI device code as reported by the machine ('2C02').
DEFINE FIELD code               ON catalog_gpu_device TYPE string                        PERMISSIONS FULL;
DEFINE FIELD device_name        ON catalog_gpu_device TYPE string DEFAULT ''             PERMISSIONS FULL;
DEFINE FIELD vendor             ON catalog_gpu_device TYPE string DEFAULT ''             PERMISSIONS FULL;
DEFINE FIELD driver             ON catalog_gpu_device TYPE none | record<catalog_driver> PERMISSIONS FULL;
DEFINE FIELD source_id          ON catalog_gpu_device TYPE none | int                    PERMISSIONS FULL;
DEFINE FIELD imported_at        ON catalog_gpu_device TYPE datetime DEFAULT time::now()  PERMISSIONS FULL;

DEFINE INDEX cgpu_code          ON catalog_gpu_device FIELDS code UNIQUE;

-- Lookup root: SMBIOS baseboard product -> its package and BIOS.
DEFINE TABLE catalog_baseboard TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- `Win32_BaseBoard.Product` verbatim ('B650 AORUS ELITE AX').
DEFINE FIELD product            ON catalog_baseboard TYPE string                         PERMISSIONS FULL;
DEFINE FIELD manufacturer       ON catalog_baseboard TYPE string DEFAULT ''              PERMISSIONS FULL;
DEFINE FIELD package            ON catalog_baseboard TYPE none | record<catalog_package> PERMISSIONS FULL;
DEFINE FIELD bios               ON catalog_baseboard TYPE none | record<catalog_bios>    PERMISSIONS FULL;
DEFINE FIELD source_id          ON catalog_baseboard TYPE none | int                     PERMISSIONS FULL;
DEFINE FIELD imported_at        ON catalog_baseboard TYPE datetime DEFAULT time::now()   PERMISSIONS FULL;

DEFINE INDEX cbb_product        ON catalog_baseboard FIELDS product UNIQUE;

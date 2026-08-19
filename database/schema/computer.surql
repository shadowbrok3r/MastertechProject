-- ============================================================
-- TABLE: computer                  last changed: Tranche 1 (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE computer TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX computer_customer_idx ON computer FIELDS customer;

DEFINE FIELD cpu               ON computer TYPE string DEFAULT '' PERMISSIONS FULL;
DEFINE FIELD current_antivirus ON computer TYPE none | array<object> PERMISSIONS FULL;
DEFINE FIELD current_antivirus.* ON computer TYPE object          PERMISSIONS FULL;
DEFINE FIELD customer          ON computer TYPE none | record<customer> PERMISSIONS FULL;
DEFINE FIELD device_mfg        ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD device_model      ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD device_name       ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD device_serial     ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD drives            ON computer TYPE array<object> DEFAULT [] PERMISSIONS FULL;
DEFINE FIELD drives.*          ON computer TYPE object           PERMISSIONS FULL;
DEFINE FIELD gpu               ON computer TYPE string DEFAULT '' PERMISSIONS FULL;
-- Defaulted so a MERGE that creates the row (the qc_fingerprint projection
-- sets no OS-side fields) does not fail the whole statement.
DEFINE FIELD hostname          ON computer TYPE string DEFAULT '' PERMISSIONS FULL;
DEFINE FIELD installed_programs   ON computer TYPE none | array<object> PERMISSIONS FULL;
DEFINE FIELD installed_programs.* ON computer TYPE object        PERMISSIONS FULL;
-- Staff/test machines; excluded from outcome rollups. NONE reads as false.
DEFINE FIELD is_internal       ON computer TYPE none | bool      PERMISSIONS FULL;
DEFINE FIELD motherboard_asset_tag ON computer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD motherboard_name  ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD motherboard_serial ON computer TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD motherboard_vendor ON computer TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD operating_system  ON computer TYPE string DEFAULT '' PERMISSIONS FULL;
DEFINE FIELD order_service     ON computer TYPE none | object    PERMISSIONS FULL;
DEFINE FIELD product_name      ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD product_serial    ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD product_sku       ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD product_vendor    ON computer TYPE none | string    PERMISSIONS FULL;
DEFINE FIELD ram               ON computer TYPE string DEFAULT '' PERMISSIONS FULL;
DEFINE FIELD seb_info          ON computer TYPE none | object    PERMISSIONS FULL;
DEFINE FIELD windows_active    ON computer TYPE none | bool      PERMISSIONS FULL;

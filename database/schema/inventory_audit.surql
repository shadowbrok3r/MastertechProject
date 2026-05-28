-- ============================================================
-- TABLE: inventory_audit          last changed: Inventory tranche
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Snapshot of a physical inventory count for one store on one
-- date. The Rust side (displays/src/tabs/stock/inventory_audit.rs)
-- imports a list of serials (CSV or paste), looks each one up in
-- Odoo (`stock.lot` + `stock.move.line`), and writes a single
-- inventory_audit row whose `serials` array holds the flattened
-- per-serial state plus the raw Odoo response so we can re-derive
-- new columns later without re-hitting the API.
--
-- During a scan walkthrough each `serials[i].found` flip is a live
-- mutation against this record, so audit progress survives restart
-- and is visible to anyone else holding the same record open.
--
-- Rust source of truth: `displays::tabs::stock::inventory_audit`.

DEFINE TABLE inventory_audit TYPE ANY SCHEMALESS PERMISSIONS FULL;

-- "Show every audit for my store, newest first" — the combobox in
-- the Store Inventory header view.
DEFINE INDEX inventory_audit_store_idx ON inventory_audit FIELDS store_id, created_at;

-- --- Store identification ---
-- Store enum tag ("RIV", "LTN", "MUR", "ORE", "SAN").
DEFINE FIELD store          ON inventory_audit TYPE string                       PERMISSIONS FULL;
-- Canonical Odoo store id (76, 73, 74, 75, 77). Used by the
-- index so we don't have to round-trip the Store enum.
DEFINE FIELD store_id       ON inventory_audit TYPE int                          PERMISSIONS FULL;

-- --- Ownership / audit trail ---
DEFINE FIELD created_by     ON inventory_audit TYPE option<record<user>>         PERMISSIONS FULL;
DEFINE FIELD created_at     ON inventory_audit TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
-- Optional display name; UI defaults to created_at when None.
DEFINE FIELD label          ON inventory_audit TYPE option<string>               PERMISSIONS FULL;

-- --- Imported serials ---
-- `serials` is the working set. Each entry is one serial that was
-- imported, with the flattened fields the UI needs to render and
-- the raw lot/move blobs to fall back on for future columns.
DEFINE FIELD serials                       ON inventory_audit TYPE array                  PERMISSIONS FULL;
-- `serials.*` and the raw_lot / raw_move blobs are intentionally
-- unconstrained. The table itself is SCHEMALESS, so SurrealDB lets these
-- object/array slots carry arbitrary subkeys (the full Odoo `stock.lot` /
-- `stock.move.line` payload, shape TBD-by-Odoo) without a FLEXIBLE
-- modifier — `FLEXIBLE` is a SCHEMAFULL-only escape hatch and would
-- parse-error here ("FLEXIBLE must be specified after TYPE").
DEFINE FIELD serials.*                     ON inventory_audit TYPE object                  PERMISSIONS FULL;
DEFINE FIELD serials.*.serial              ON inventory_audit TYPE string                  PERMISSIONS FULL;
DEFINE FIELD serials.*.item_code           ON inventory_audit TYPE option<string>          PERMISSIONS FULL;
DEFINE FIELD serials.*.product_name        ON inventory_audit TYPE option<string>          PERMISSIONS FULL;
DEFINE FIELD serials.*.last_location       ON inventory_audit TYPE option<string>          PERMISSIONS FULL;
DEFINE FIELD serials.*.last_reference      ON inventory_audit TYPE option<string>          PERMISSIONS FULL;
DEFINE FIELD serials.*.last_move_date      ON inventory_audit TYPE option<string>          PERMISSIONS FULL;
DEFINE FIELD serials.*.std_price           ON inventory_audit TYPE option<float>           PERMISSIONS FULL;
DEFINE FIELD serials.*.list_price          ON inventory_audit TYPE option<float>           PERMISSIONS FULL;
DEFINE FIELD serials.*.found               ON inventory_audit TYPE bool DEFAULT false      PERMISSIONS FULL;
DEFINE FIELD serials.*.raw_lot             ON inventory_audit TYPE option<object>          PERMISSIONS FULL;
DEFINE FIELD serials.*.raw_move            ON inventory_audit TYPE option<array>           PERMISSIONS FULL;

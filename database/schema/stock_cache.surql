-- ============================================================
-- TABLE: stock_cache               added: Odoo stock cache (2026-07-10)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Shared cache for Odoo stock pulls (store_stock / serials per Odoo
-- location, extra_info globally). Clients read `data` when
-- `refreshed_at` is fresher than 1 day; the first client to find it
-- stale claims `refreshing_since` + `claim_token` and performs the
-- single Odoo fetch for everyone. Writes and releases are scoped to
-- the claim token, so a DELETE-invalidated or taken-over claim cannot
-- resurrect stale data. The stock tab's Refresh buttons force a re-fetch.
-- Record ids: store_stock_<loc> / serials_<loc> / extra_info.

DEFINE TABLE stock_cache TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD kind             ON stock_cache TYPE string           PERMISSIONS FULL;
DEFINE FIELD location_id      ON stock_cache TYPE none | int       PERMISSIONS FULL;
DEFINE FIELD data             ON stock_cache TYPE none | array     PERMISSIONS FULL;
DEFINE FIELD refreshed_at     ON stock_cache TYPE none | datetime  PERMISSIONS FULL;
DEFINE FIELD refreshing_since ON stock_cache TYPE none | datetime  PERMISSIONS FULL;
DEFINE FIELD claim_token      ON stock_cache TYPE none | string    PERMISSIONS FULL;

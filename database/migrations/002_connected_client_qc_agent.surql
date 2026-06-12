-- ============================================================
-- Migration 002 — connected_client.client_kind: allow 'qc_agent'
-- Applied: (pending)
-- Safe to re-run: YES (DEFINE FIELD OVERWRITE is idempotent)
-- ============================================================
-- The running DB predates ClientKind::QcAgent (pre-OS UEFI fingerprint
-- agents), so its field assert still reads IN ['machine', 'build_worker']
-- and qc_fingerprint connected_client upserts fail with:
--   "Found 'qc_agent' for field `client_kind` ... must conform to:
--    $value INSIDE ['machine', 'build_worker']"
-- This re-applies the definition from database/schema/connected_client.surql.

DEFINE FIELD OVERWRITE client_kind ON connected_client TYPE string DEFAULT 'machine' ASSERT $value IN ['machine', 'build_worker', 'qc_agent'] PERMISSIONS FULL;

-- ============================================================
-- Migration 001 — Tranche 1: relax required FK fields
-- Applied: 2026-05-16
-- Safe to re-run: YES (DEFINE FIELD is idempotent)
-- ============================================================
-- Audit findings that drive these changes:
--   computer.customer     : 24 orphans, 6 UUID garbage from random_record_id()
--   service_order.customer: 45 orphans, 11 UUID garbage from random_record_id()
--   connected_client.assigned_user: Rust struct uses Option<RecordId> but
--     schema was record<user> (required) — latent write-time landmine
--   computer.current_antivirus: schema was array<string> but Rust now writes
--     InstalledSecurityProduct objects; no existing data so safe to change
-- ============================================================

-- 1. computer.customer — required → optional
--    Allows the Rust ComputerData struct (customer: Option<RecordId>) to write
--    NONE without a schema rejection. Existing rows are unaffected.
DEFINE FIELD customer ON computer TYPE none | record<customer> PERMISSIONS FULL;

-- 2. service_order.customer — required → optional
--    TicketData::default() previously wrote random_record_id(CUSTOMER_TABLE).
--    Once the Rust side switches to Option<RecordId> the field will be NONE
--    for new scratch records.
DEFINE FIELD customer ON service_order TYPE none | record<customer> PERMISSIONS FULL;

-- 3. connected_client.assigned_user — required → optional
--    ConnectedClient struct already declares assigned_user: Option<RecordId>.
--    Schema now matches. All 1,582 existing rows have the field set so there
--    is zero data impact.
DEFINE FIELD assigned_user ON connected_client TYPE none | record<user> PERMISSIONS FULL;

-- 4a. computer.current_antivirus — array<string> → array<object>
--     The InstalledSecurityProduct struct serializes as a JSON object.
--     All 2,331 existing rows have an empty array or NULL; no data migration
--     required. Legacy string rows (none currently) would still deserialize
--     via the custom deserializer on the Rust side.
DEFINE FIELD current_antivirus ON computer TYPE none | array<object> PERMISSIONS FULL;

-- 4b. element constraint update
DEFINE FIELD current_antivirus.* ON computer TYPE object PERMISSIONS FULL;

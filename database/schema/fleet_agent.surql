-- ============================================================
-- TABLE: fleet_agent              last changed: Fleet tranche (2026-05-21)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One row per qc-app instance the orchestrator has ever seen.
-- Identified by the same `machine_id` the agent SHA-256s out of
-- hostname + CPU brand, so the deterministic record id is
-- `fleet_agent:<machine_id>` and upserts are idempotent.
--
-- `axum_server::routes::api::qc_fleet` is the hot-path writer
-- (in-memory `FleetState` for live requests, fire-and-forget DB
-- mirror via `mirror_agent_upsert`). On startup `hydrate_from_db`
-- re-populates `FleetState` from this table so server restarts
-- don't black-hole the admin dashboard.
--
-- Rust source of truth: `database::schema::fleet::FleetAgent`.

DEFINE TABLE fleet_agent TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

-- "Is this machine online" queries hit `last_heartbeat`. The
-- heartbeat sweep cron job (60 s cadence) scans this index to
-- flag stale agents within ~3 min of the last beat.
DEFINE INDEX fagent_last_heartbeat ON fleet_agent FIELDS last_heartbeat;

-- Filter agents by version (rolling update detection).
DEFINE INDEX fagent_agent_version  ON fleet_agent FIELDS agent_version;

-- Filter by hostname when available (operator-friendly grouping).
DEFINE INDEX fagent_hostname       ON fleet_agent FIELDS hostname;

-- --- Identification ---
DEFINE FIELD machine_id      ON fleet_agent TYPE string                    PERMISSIONS FULL;
DEFINE FIELD agent_version   ON fleet_agent TYPE string                    PERMISSIONS FULL;
DEFINE FIELD hostname        ON fleet_agent TYPE none | string             PERMISSIONS FULL;

-- --- Lifecycle timestamps ---
DEFINE FIELD registered_at   ON fleet_agent TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD last_heartbeat  ON fleet_agent TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD last_report_at  ON fleet_agent TYPE none | datetime           PERMISSIONS FULL;

-- --- Latest hot-path stats (denormalized from fleet_event) ---
-- Mean CPU% from the most recent heartbeat. 0..=100.
DEFINE FIELD cpu_avg_pct     ON fleet_agent TYPE number DEFAULT 0          PERMISSIONS FULL;

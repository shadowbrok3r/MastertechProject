-- ============================================================
-- TABLE: fleet_command            last changed: Fleet tranche (2026-05-21)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Mirror of every command the orchestrator has dispatched to an
-- agent. The live queue lives in-memory inside `axum_server`'s
-- `FleetState` (so a long agent poll doesn't beg a DB call), but
-- every issue + ack also writes here so the admin UI can answer
-- "what did we ask machine X to do this week" without scraping
-- the audit log.
--
-- `external_id` is the short HTTP-visible id the agent quotes in
-- its ack POST — distinct from the row id (which is a random
-- `fleet_command:<uuid>`).
--
-- Rust source of truth: `database::schema::fleet::FleetCommand`.

DEFINE TABLE fleet_command TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

-- "Show every command issued to this agent" — primary dashboard query.
DEFINE INDEX fcmd_agent_ref       ON fleet_command FIELDS agent_ref;

-- Cross-agent filter by machine_id when the caller only has the id string.
DEFINE INDEX fcmd_machine_id      ON fleet_command FIELDS machine_id;

-- Looked up by the ack handler to flip status pending→acknowledged.
-- Not UNIQUE because forward-compat could allow re-issuing the same id.
DEFINE INDEX fcmd_external_id     ON fleet_command FIELDS external_id;

-- Filter by status (operator hunting for stuck/failed commands).
DEFINE INDEX fcmd_status          ON fleet_command FIELDS status;

-- Issued-time scans for the per-week history view.
DEFINE INDEX fcmd_issued_at       ON fleet_command FIELDS issued_at;

-- --- Links ---
DEFINE FIELD agent_ref       ON fleet_command TYPE record<fleet_agent>     PERMISSIONS FULL;

-- --- Identification (denormalized for index scans) ---
DEFINE FIELD machine_id      ON fleet_command TYPE string                  PERMISSIONS FULL;

-- --- External (HTTP) id the agent quotes in its ack ---
DEFINE FIELD external_id     ON fleet_command TYPE string                  PERMISSIONS FULL;

-- --- Command type ---
-- One of: 'send_report' | 'run_stress_scenario' | 'custom'.
-- Mirrors `database::schema::fleet::FleetCommandKind`.
DEFINE FIELD kind            ON fleet_command TYPE string                  PERMISSIONS FULL;

-- --- Lifecycle status ---
-- One of: 'pending' | 'acknowledged' | 'failed'. `failed` is reserved
-- for an explicit failure ack from the agent (not yet wired).
DEFINE FIELD status          ON fleet_command TYPE string DEFAULT 'pending' PERMISSIONS FULL;

-- --- Timing ---
DEFINE FIELD issued_at       ON fleet_command TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD acked_at        ON fleet_command TYPE none | datetime         PERMISSIONS FULL;

-- --- Optional opaque payload (Custom commands stash JSON here) ---
DEFINE FIELD payload         ON fleet_command TYPE none | object           PERMISSIONS FULL;

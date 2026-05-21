-- ============================================================
-- TABLE: fleet_event              last changed: Fleet tranche (2026-05-21)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Append-only audit log for fleet activity: register, heartbeat,
-- report upload, command issued, command acknowledged. Every
-- `qc_fleet` handler writes one row here after the in-memory
-- mutation so the operator can correlate a flaky agent with the
-- exact sequence of events that led to it.
--
-- Rows are never updated. Old rows can be aged off by an external
-- cron job once they pass the retention window; the table itself
-- is intentionally unbounded so we don't lose data inside the cron
-- window.
--
-- Rust source of truth: `database::schema::fleet::FleetEvent`.

DEFINE TABLE fleet_event TYPE ANY SCHEMALESS CHANGEFEED 7d PERMISSIONS FULL;

-- "Show every event for this agent, newest first" — the dashboard's
-- per-machine timeline view.
DEFINE INDEX fevent_agent_ref     ON fleet_event FIELDS agent_ref;

-- Cross-agent timeline filtered by machine_id string (used when the
-- caller has the id but not the record link).
DEFINE INDEX fevent_machine_id    ON fleet_event FIELDS machine_id;

-- Filter by event kind ("show me every report this week").
DEFINE INDEX fevent_kind          ON fleet_event FIELDS kind;

-- Time-window scans for the audit page (latest 500 entries, etc).
DEFINE INDEX fevent_at            ON fleet_event FIELDS at;

-- --- Links ---
DEFINE FIELD agent_ref       ON fleet_event TYPE record<fleet_agent>       PERMISSIONS FULL;

-- --- Identification (denormalized for index scans) ---
DEFINE FIELD machine_id      ON fleet_event TYPE string                    PERMISSIONS FULL;

-- --- Event type ---
-- One of: 'register' | 'heartbeat' | 'report' | 'command_issued' | 'command_acked'.
-- Mirrors `database::schema::fleet::FleetEventKind`.
DEFINE FIELD kind            ON fleet_event TYPE string                    PERMISSIONS FULL;

-- --- Timing ---
DEFINE FIELD at              ON fleet_event TYPE datetime DEFAULT time::now() PERMISSIONS FULL;

-- --- Structured payload (heartbeat cpu, report excerpt, command kind, etc) ---
DEFINE FIELD payload         ON fleet_event TYPE none | object             PERMISSIONS FULL;

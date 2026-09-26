-- ============================================================
-- Migration 003 — agent approve-all
-- Applied: 2026-09-26 (surrealdb-dev, root /sql)
-- Safe to re-run: YES (DEFINE FIELD OVERWRITE is idempotent)
-- ============================================================
-- The start phase of database/rollouts/20260926030000__agent_approve_all.toml,
-- for a plain root /sql call:
--   agent_approval.status admits 'accepted_all_for_session';
--   agent_turn.kind admits 'approvals';
--   agent_thread.approve_all is an optional bool record users can read but not write.
-- Matches database/schema/agent_approval.surql, agent_turn.surql and agent_thread.surql.

DEFINE FIELD OVERWRITE status ON agent_approval TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'accepted', 'accepted_for_session', 'accepted_all_for_session', 'declined', 'cancelled', 'answered', 'expired', 'auto_declined', 'auto_accepted', 'resolved_elsewhere', 'failed'] PERMISSIONS FULL;
DEFINE FIELD OVERWRITE kind ON agent_turn TYPE string DEFAULT 'start'
    ASSERT $value IN ['start', 'steer', 'interrupt', 'close', 'queue', 'compact', 'rename', 'approvals'] PERMISSIONS FULL;
DEFINE FIELD OVERWRITE approve_all ON agent_thread TYPE none | bool
    PERMISSIONS FOR select FULL, FOR create, update NONE;

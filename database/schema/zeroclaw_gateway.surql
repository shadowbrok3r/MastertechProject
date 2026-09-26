-- ============================================================
-- TABLE: zeroclaw_gateway
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- The ZeroClaw gateway Root users browse agent sessions and automations through.
-- Only an active Root user may read it; only a system user writes it.

DEFINE TABLE zeroclaw_gateway TYPE NORMAL SCHEMAFULL
    PERMISSIONS FOR select WHERE $access = 'user' AND $auth.id != NONE AND $auth.active = true AND $auth.authorization = 'Root', FOR create, update, delete NONE;

DEFINE FIELD token      ON zeroclaw_gateway TYPE string                       PERMISSIONS FULL;
DEFINE FIELD updated_at ON zeroclaw_gateway TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD url        ON zeroclaw_gateway TYPE string                       PERMISSIONS FULL;

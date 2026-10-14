-- ============================================================
-- TABLE: admin_action
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Append-only log of account changes made from the User Admin tab; Root only.

DEFINE TABLE admin_action TYPE NORMAL SCHEMAFULL
    PERMISSIONS
        FOR select, create WHERE $access = 'user' AND $auth.authorization = 'Root' AND $auth.active = true,
        FOR update, delete NONE;

DEFINE FIELD actor      ON admin_action TYPE record<user> VALUE $before ?? $auth.id;
DEFINE FIELD target     ON admin_action TYPE record<user>;
DEFINE FIELD action     ON admin_action TYPE string;
DEFINE FIELD detail     ON admin_action TYPE object FLEXIBLE DEFAULT {};
DEFINE FIELD created_at ON admin_action TYPE datetime VALUE $before ?? time::now();

DEFINE INDEX admin_action_target ON admin_action FIELDS target, created_at;

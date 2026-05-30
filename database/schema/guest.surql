-- ============================================================
-- TABLE: guest
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE guest TYPE NORMAL SCHEMAFULL
    PERMISSIONS
        FOR select, update, delete WHERE $access = 'guest' AND id = $auth.id,
        FOR create NONE;

DEFINE FIELD password ON guest TYPE string PERMISSIONS FULL;
DEFINE FIELD username ON guest TYPE string PERMISSIONS FULL;

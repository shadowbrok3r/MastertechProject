-- ============================================================
-- TABLE: password_reset
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One-time reset codes: Root issues for anyone else, a Manager for User/Warehouse accounts in their store.
-- Redeemed only through the `password_reset` access (database/access/password_reset.surql).

DEFINE TABLE password_reset TYPE NORMAL SCHEMAFULL
    PERMISSIONS
        FOR select WHERE $access = 'user' AND $auth.active = true AND ($auth.authorization = 'Root' OR issued_by = $auth.id),
        FOR create WHERE $access = 'user' AND $auth.active = true AND user != $auth.id
            AND ($auth.authorization = 'Root'
                OR ($auth.authorization = 'Manager' AND user.store = $auth.store AND user.authorization NOT IN ['Root', 'Manager'])),
        FOR update NONE,
        FOR delete WHERE $access = 'user' AND $auth.active = true AND ($auth.authorization = 'Root' OR issued_by = $auth.id);

DEFINE FIELD user       ON password_reset TYPE record<user>;
DEFINE FIELD issued_by  ON password_reset TYPE record<user> VALUE $before ?? $auth.id;
DEFINE FIELD code_hash  ON password_reset TYPE string PERMISSIONS FOR select NONE, FOR create, update FULL;
DEFINE FIELD created_at ON password_reset TYPE datetime VALUE $before ?? time::now();
DEFINE FIELD expires_at ON password_reset TYPE datetime VALUE $before ?? (time::now() + 1h);
DEFINE FIELD attempts   ON password_reset TYPE int DEFAULT 0 ASSERT $value >= 0;
DEFINE FIELD used_at    ON password_reset TYPE none | datetime;

DEFINE INDEX password_reset_user ON password_reset FIELDS user, created_at;

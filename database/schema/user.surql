-- ============================================================
-- TABLE: user                     last changed: user_mcp_settings
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- SCHEMAFULL. Rust source of truth: `database::schema::user::User`.
-- Writes are field-targeted (UPDATE $auth.id SET <field> = ...),
-- never full-record, so struct-only fields without a DEFINE here
-- are tolerated but cannot be persisted until defined.

DEFINE TABLE user TYPE ANY SCHEMAFULL
    PERMISSIONS
        FOR select, create, update FULL,
        FOR delete WHERE $access = 'users' AND id = $auth.id;

DEFINE INDEX presta_idx      ON user FIELDS id_prestashop UNIQUE;
DEFINE INDEX userEmailIndex  ON user FIELDS email UNIQUE;

DEFINE FIELD active            ON user TYPE bool   DEFAULT true                          PERMISSIONS FULL;
DEFINE FIELD authorization     ON user TYPE string DEFAULT 'User'                        PERMISSIONS FULL;
DEFINE FIELD email             ON user TYPE string ASSERT string::is_email($value)       PERMISSIONS FULL;
DEFINE FIELD everest_initials  ON user TYPE string                                       PERMISSIONS FULL;
DEFINE FIELD id_prestashop     ON user TYPE none | number DEFAULT 0                      PERMISSIONS FULL;
DEFINE FIELD id_store          ON user TYPE none | string DEFAULT ''                     PERMISSIONS FULL;
DEFINE FIELD minio_access_key  ON user TYPE none | string DEFAULT NONE                   PERMISSIONS FULL;
DEFINE FIELD minio_secret_key  ON user TYPE none | string DEFAULT NONE                   PERMISSIONS FULL;
DEFINE FIELD name              ON user TYPE string                                       PERMISSIONS FULL;
DEFINE FIELD password          ON user TYPE string                                       PERMISSIONS FULL;
DEFINE FIELD store             ON user TYPE string                                       PERMISSIONS FULL;
DEFINE FIELD version           ON user TYPE string                                       PERMISSIONS FULL;

-- OpenAI-compatible MCP endpoint config edited from the Account Settings page.
DEFINE FIELD mcp_settings          ON user TYPE object DEFAULT {}     PERMISSIONS FULL;
DEFINE FIELD mcp_settings.endpoint ON user TYPE none | string         PERMISSIONS FULL;
DEFINE FIELD mcp_settings.api_key  ON user TYPE none | string         PERMISSIONS FULL;
DEFINE FIELD mcp_settings.model    ON user TYPE none | string         PERMISSIONS FULL;

DEFINE FIELD user_settings ON user TYPE object FLEXIBLE
    DEFAULT { color_scheme: {  }, ui_layout: { mastertech: {  }, mtechserver: {  } } } PERMISSIONS FULL;
DEFINE FIELD user_settings.color_scheme         ON user TYPE none | bytes                               PERMISSIONS FULL;
DEFINE FIELD user_settings.ui_layout            ON user TYPE none | object FLEXIBLE
    DEFAULT { mastertech: {  }, mtechserver: {  } } PERMISSIONS FULL;
DEFINE FIELD user_settings.ui_layout.mastertech  ON user TYPE none | object FLEXIBLE DEFAULT {  }        PERMISSIONS FULL;
DEFINE FIELD user_settings.ui_layout.mtechserver ON user TYPE none | object FLEXIBLE DEFAULT {  }        PERMISSIONS FULL;

DEFINE FIELD user_statuses    ON user TYPE none | array<string> DEFAULT []               PERMISSIONS FULL;
DEFINE FIELD user_statuses.*  ON user TYPE string                                        PERMISSIONS FULL;

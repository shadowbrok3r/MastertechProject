-- ============================================================
-- TABLE: user                     last changed: ai_assistant_tools
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- SCHEMAFULL. Rust source of truth: `database::schema::user::User`.
-- Writes are field-targeted (UPDATE $auth.id SET <field> = ...),
-- never full-record, so struct-only fields without a DEFINE here
-- are tolerated but cannot be persisted until defined.
--
-- Record access is `user` (USER_SCOPE). Admin = active `Root`, matching User::is_admin.
-- SIGNIN/SIGNUP clauses run as a database Editor and bypass these rules.
-- A denied field write is reverted to the stored value, not rejected.

DEFINE TABLE user TYPE ANY SCHEMAFULL
    PERMISSIONS
        FOR select WHERE $access = 'user',
        FOR create WHERE $access = 'user' AND $auth.authorization = 'Root' AND $auth.active = true,
        FOR update WHERE $access = 'user' AND (id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true)),
        FOR delete NONE;

-- Admin-managed: role, status, identity. `$auth.*` reads the in-flight row on a self-edit,
-- so these also exclude the admin's own row.
DEFINE FIELD active            ON user TYPE bool   DEFAULT true
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD authorization     ON user TYPE string DEFAULT 'User'
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD email             ON user TYPE string ASSERT string::is_email($value)
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD everest_initials  ON user TYPE string
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD id_prestashop     ON user TYPE none | number DEFAULT NONE
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD id_store          ON user TYPE none | string DEFAULT ''
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD name              ON user TYPE string
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD store             ON user TYPE string
    PERMISSIONS FOR select, create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;

-- Secrets: MinIO keys readable by the owner only, the password hash by no record user.
DEFINE FIELD minio_access_key  ON user TYPE none | string DEFAULT NONE
    PERMISSIONS FOR select WHERE id = $auth.id, FOR create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD minio_secret_key  ON user TYPE none | string DEFAULT NONE
    PERMISSIONS FOR select WHERE id = $auth.id, FOR create FULL, FOR update WHERE id != $auth.id AND $auth.authorization = 'Root' AND $auth.active = true;
DEFINE FIELD password          ON user TYPE string
    PERMISSIONS FOR select NONE, FOR create FULL, FOR update WHERE id = $auth.id;

-- Owner-managed.
DEFINE FIELD version           ON user TYPE string
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;

-- OpenAI-compatible MCP endpoint config edited from the Account Settings page.
DEFINE FIELD mcp_settings          ON user TYPE object DEFAULT {}
    PERMISSIONS FOR select WHERE id = $auth.id, FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD mcp_settings.endpoint ON user TYPE none | string
    PERMISSIONS FOR select WHERE id = $auth.id, FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD mcp_settings.api_key  ON user TYPE none | string
    PERMISSIONS FOR select WHERE id = $auth.id, FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD mcp_settings.model    ON user TYPE none | string
    PERMISSIONS FOR select WHERE id = $auth.id, FOR create FULL, FOR update WHERE id = $auth.id;

DEFINE FIELD user_settings ON user TYPE object FLEXIBLE
    DEFAULT { color_scheme: {  }, ui_layout: { mastertech: {  }, mtechserver: {  } } }
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD user_settings.color_scheme         ON user TYPE none | bytes
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD user_settings.tui_color_scheme     ON user TYPE none | bytes
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD user_settings.ui_layout            ON user TYPE none | object FLEXIBLE
    DEFAULT { mastertech: {  }, mtechserver: {  } }
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD user_settings.ui_layout.mastertech  ON user TYPE none | object FLEXIBLE DEFAULT {  }
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD user_settings.ui_layout.mtechserver ON user TYPE none | object FLEXIBLE DEFAULT {  }
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
-- Which work mode MasterTech opens into. No DEFAULT: NONE means show the picker,
-- and an added non-none field without a backfill fails coercion on every old row.
DEFINE FIELD user_settings.ui_layout.default_work_mode ON user TYPE none | string
    ASSERT $value == NONE OR $value IN ['tuneup_qc', 'diagnostic', 'admin', 'full']
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;

-- The owner's AI assistant persona, injected into their agent sessions.
DEFINE FIELD ai_profile                ON user TYPE none | object
    PERMISSIONS FOR select WHERE id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true), FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD ai_profile.assistant_name ON user TYPE none | string ASSERT $value == NONE OR string::len($value) <= 40
    PERMISSIONS FOR select WHERE id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true), FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD ai_profile.personality    ON user TYPE none | string ASSERT $value == NONE OR string::len($value) <= 300
    PERMISSIONS FOR select WHERE id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true), FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD ai_profile.detail         ON user TYPE none | string ASSERT $value == NONE OR $value IN ['brief', 'normal', 'detailed']
    PERMISSIONS FOR select WHERE id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true), FOR create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD ai_profile.about_me       ON user TYPE none | string ASSERT $value == NONE OR string::len($value) <= 500
    PERMISSIONS FOR select WHERE id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true), FOR create FULL, FOR update WHERE id = $auth.id;
-- NONE means on.
DEFINE FIELD ai_profile.morning_brief  ON user TYPE none | bool
    PERMISSIONS FOR select WHERE id = $auth.id OR ($auth.authorization = 'Root' AND $auth.active = true), FOR create FULL, FOR update WHERE id = $auth.id;

DEFINE FIELD user_statuses    ON user TYPE none | array<string> DEFAULT []
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;
DEFINE FIELD user_statuses.*  ON user TYPE string
    PERMISSIONS FOR select, create FULL, FOR update WHERE id = $auth.id;

-- Indexes come after the fields they cover: a file is applied in order, and an
-- index on a not-yet-defined field aborts the sync.
-- UNIQUE permits many NONE rows but still rejects a duplicate real id, so
-- staff without a PrestaShop id must store NONE and never 0.
DEFINE INDEX presta_idx      ON user FIELDS id_prestashop UNIQUE;
DEFINE INDEX userEmailIndex  ON user FIELDS email UNIQUE;

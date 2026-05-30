-- ============================================================
-- TABLE: plugin_registry
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE plugin_registry TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD author             ON plugin_registry TYPE none | string                   PERMISSIONS FULL;
DEFINE FIELD created_at         ON plugin_registry TYPE datetime DEFAULT time::now()     PERMISSIONS FULL;
DEFINE FIELD description        ON plugin_registry TYPE string                           PERMISSIONS FULL;
DEFINE FIELD name               ON plugin_registry TYPE string                           PERMISSIONS FULL;
DEFINE FIELD plugin_id          ON plugin_registry TYPE string                           PERMISSIONS FULL;
DEFINE FIELD source_code        ON plugin_registry TYPE none | string                    PERMISSIONS FULL;
DEFINE FIELD tags               ON plugin_registry TYPE array<string>                    PERMISSIONS FULL;
DEFINE FIELD tags.*             ON plugin_registry TYPE string                           PERMISSIONS FULL;
DEFINE FIELD tools              ON plugin_registry TYPE array<object>                    PERMISSIONS FULL;
DEFINE FIELD tools.*            ON plugin_registry TYPE object                           PERMISSIONS FULL;
DEFINE FIELD tools.*.description ON plugin_registry TYPE string                          PERMISSIONS FULL;
DEFINE FIELD updated_at         ON plugin_registry TYPE datetime DEFAULT time::now()     PERMISSIONS FULL;
DEFINE FIELD version            ON plugin_registry TYPE string                           PERMISSIONS FULL;
DEFINE FIELD wasm_bucket_path   ON plugin_registry TYPE none | string                    PERMISSIONS FULL;

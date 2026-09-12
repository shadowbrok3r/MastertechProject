-- diagnostic_entry — AI/tech notes against a diagnostic_session.
--
-- Rust source of truth: `database::schema::DiagnosticEntry`.
-- Table is created lazily on first insert (no DEFINE TABLE here), which is what
-- prod holds: `TYPE ANY SCHEMALESS PERMISSIONS FULL`.
-- `embedding` is set by the Rust app via fn::embed_text(title + detail) at
-- write time. No DB-side VALUE/event: a VALUE clause recomputes on every write
-- (a synchronous fn::embed_text HTTP call) and overrides the app's value, so it
-- lives in the app, not the schema. Matches deployed prod.
--
-- Every field below was already defined in prod but recorded in no schema file,
-- so a rebuild from these files would have produced a table with only
-- `embedding` typed. Captured verbatim from `INFO FOR TABLE diagnostic_entry`;
-- applying this file re-asserts what is already there.

DEFINE FIELD OVERWRITE session_ref ON diagnostic_entry TYPE record<diagnostic_session> PERMISSIONS FULL;
DEFINE FIELD OVERWRITE timestamp ON diagnostic_entry TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD OVERWRITE category ON diagnostic_entry TYPE string PERMISSIONS FULL;
DEFINE FIELD OVERWRITE title ON diagnostic_entry TYPE string PERMISSIONS FULL;
DEFINE FIELD OVERWRITE detail ON diagnostic_entry TYPE string PERMISSIONS FULL;
DEFINE FIELD OVERWRITE data ON diagnostic_entry TYPE none | object PERMISSIONS FULL;
DEFINE FIELD OVERWRITE plugins_used ON diagnostic_entry TYPE array<object> PERMISSIONS FULL;
DEFINE FIELD OVERWRITE plugins_used.* ON diagnostic_entry TYPE object PERMISSIONS FULL;

DEFINE FIELD OVERWRITE embedding ON diagnostic_entry TYPE array<float> PERMISSIONS FULL;
DEFINE FIELD OVERWRITE embedding.* ON diagnostic_entry TYPE float PERMISSIONS FULL;

-- Index last: a file is applied in order and an index on a not-yet-defined
-- field aborts the sync. LM is auto-derived by SurrealDB and is not written here.
DEFINE INDEX OVERWRITE diag_embedding ON diagnostic_entry FIELDS embedding
  HNSW DIMENSION 768 DIST COSINE TYPE F32 EFC 150 M 12 M0 24;

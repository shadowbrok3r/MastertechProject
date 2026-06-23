-- diagnostic_entry — AI/tech notes against a diagnostic_session.
--
-- Rust source of truth: `database::schema::DiagnosticEntry`.
-- Table is created lazily on first insert (no DEFINE TABLE here).
-- `embedding` is set by the Rust app via fn::embed_text(title + detail) at
-- write time. No DB-side VALUE/event: a VALUE clause recomputes on every write
-- (a synchronous fn::embed_text HTTP call) and overrides the app's value, so it
-- lives in the app, not the schema. Matches deployed prod.

DEFINE FIELD OVERWRITE embedding ON diagnostic_entry TYPE array<float> PERMISSIONS FULL;
DEFINE FIELD OVERWRITE embedding.* ON diagnostic_entry TYPE float PERMISSIONS FULL;

DEFINE INDEX OVERWRITE diag_embedding ON diagnostic_entry FIELDS embedding
  HNSW DIMENSION 768 DIST COSINE TYPE F32 EFC 150 M 12 M0 24;

-- diagnostic_entry — AI/tech notes against a diagnostic_session.
--
-- Rust source of truth: `database::schema::DiagnosticEntry`.
-- Table is created lazily on first insert (no DEFINE TABLE here).
--
-- Embeddings: `DiagnosticEntry::create` sets `embedding` via
-- `fn::embed_text(title + detail)` in the same CREATE. The FIELD `VALUE`
-- below covers ad-hoc inserts that omit `embedding`. A CREATE-time EVENT
-- cannot rescue rows where the client explicitly sends `embedding: []` —
-- the HNSW index rejects dimension 0 before the event runs.

DEFINE FIELD OVERWRITE embedding ON diagnostic_entry TYPE array<float>
  VALUE fn::embed_text(string::concat(title ?? '', ' ', detail ?? ''))
  PERMISSIONS FULL;
DEFINE FIELD OVERWRITE embedding.* ON diagnostic_entry TYPE float PERMISSIONS FULL;

DEFINE INDEX OVERWRITE diag_embedding ON diagnostic_entry FIELDS embedding
  HNSW DIMENSION 768 DIST COSINE TYPE F32 EFC 150 M 12 M0 24;

DEFINE EVENT OVERWRITE diagentry_reembed ON diagnostic_entry
  WHEN $event = "UPDATE"
    AND ($before.title != $after.title OR $before.detail != $after.detail)
  THEN {
    UPDATE $after.id SET embedding = fn::embed_text(string::concat($after.title, ' ', $after.detail));
  };

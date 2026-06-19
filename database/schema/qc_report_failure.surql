-- ============================================================
-- TABLE: qc_report_failure         last changed: Checklist tranche (2026-06-18)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One row per failed checklist item at sign-off, linked to its qc_report
-- via report_ref, for post-ship accountability (which item failed, on which
-- order, with the tech's note). Mirrors QCWizard qc_sign_off_failure.
-- Rust source of truth: `database::orders::QcReportFailureRecord`.

DEFINE TABLE qc_report_failure TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

DEFINE INDEX qcfail_order_id   ON qc_report_failure FIELDS failure.order_id;
DEFINE INDEX qcfail_item_key   ON qc_report_failure FIELDS failure.item_key;
DEFINE INDEX qcfail_report_ref ON qc_report_failure FIELDS report_ref;
DEFINE INDEX qcfail_created_at ON qc_report_failure FIELDS created_at;

DEFINE FIELD report_ref ON qc_report_failure TYPE string   PERMISSIONS FULL;
DEFINE FIELD created_at ON qc_report_failure TYPE datetime PERMISSIONS FULL;
DEFINE FIELD failure    ON qc_report_failure TYPE object   PERMISSIONS FULL;

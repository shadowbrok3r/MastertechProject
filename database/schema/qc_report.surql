-- ============================================================
-- TABLE: qc_report                 last changed: Order-QC tranche (2026-06-10)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- One bench QC sign-off for one order: verdict + error counters +
-- spec-check summary + the qc_sign_off-style checklist, with a
-- run_ref string pointing at the backing stress_test_run. Replaces
-- the MySQL qc_wizard sign-off tables for new submissions.
-- Rust source of truth: `database::orders::QcReportRecord`.

DEFINE TABLE qc_report TYPE ANY SCHEMALESS CHANGEFEED 30d PERMISSIONS FULL;

-- Pull every bench report for an order / machine / tech.
DEFINE INDEX qcrep_order_id   ON qc_report FIELDS report.order_id;
DEFINE INDEX qcrep_backend    ON qc_report FIELDS report.backend;
DEFINE INDEX qcrep_machine    ON qc_report FIELDS report.machine_id;
DEFINE INDEX qcrep_verdict    ON qc_report FIELDS report.verdict;
DEFINE INDEX qcrep_created_at ON qc_report FIELDS created_at;

DEFINE FIELD created_at ON qc_report TYPE datetime PERMISSIONS FULL;
DEFINE FIELD report     ON qc_report TYPE object   PERMISSIONS FULL;

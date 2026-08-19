-- ============================================================
-- TABLE: assist_request
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- Tech-confirmed AI assistance queue. The bench client CREATEs a row on
-- Confirm; the headless agent claims pending -> dispatched and calls the
-- zeroclaw gateway. Identity fields default from $auth so the signed-in tech
-- is the recorded requester regardless of what the client sent.
-- ============================================================

DEFINE TABLE assist_request TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD agent              ON assist_request TYPE none | string PERMISSIONS FULL;
DEFINE FIELD computer           ON assist_request TYPE none | record<computer> PERMISSIONS FULL;
DEFINE FIELD connection_string  ON assist_request TYPE string PERMISSIONS FULL;
DEFINE FIELD created_at         ON assist_request TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD customer           ON assist_request TYPE none | record<customer> PERMISSIONS FULL;
DEFINE FIELD dispatch_error     ON assist_request TYPE none | string PERMISSIONS FULL;
DEFINE FIELD dispatched_at      ON assist_request TYPE none | datetime PERMISSIONS FULL;
DEFINE FIELD finished_at        ON assist_request TYPE none | datetime PERMISSIONS FULL;
DEFINE FIELD hostname           ON assist_request TYPE none | string PERMISSIONS FULL;
-- Tech affirmed this machine is the one on that service order; ground truth.
DEFINE FIELD machine_confirmed  ON assist_request TYPE bool DEFAULT false PERMISSIONS FULL;
DEFINE FIELD requested_by       ON assist_request TYPE none | string DEFAULT $auth.email PERMISSIONS FULL;
DEFINE FIELD service_number     ON assist_request TYPE none | string PERMISSIONS FULL;
DEFINE FIELD service_order      ON assist_request TYPE none | record<service_order> PERMISSIONS FULL;
DEFINE FIELD status             ON assist_request TYPE string DEFAULT 'pending'
    ASSERT $value IN ['pending', 'dispatched', 'completed', 'failed', 'declined'] PERMISSIONS FULL;
DEFINE FIELD store              ON assist_request TYPE none | string PERMISSIONS FULL;
DEFINE FIELD tech_note          ON assist_request TYPE none | string
    ASSERT $value == NONE OR string::len($value) <= 500 PERMISSIONS FULL;
DEFINE FIELD trigger_source     ON assist_request TYPE string DEFAULT 'tur_sheet'
    ASSERT $value IN ['auto', 'tur_sheet'] PERMISSIONS FULL;

DEFINE INDEX assist_request_status_idx ON assist_request FIELDS status;

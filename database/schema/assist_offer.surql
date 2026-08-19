-- ============================================================
-- TABLE: assist_offer
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- Written by the headless agent when it resolves a connected machine to an
-- open service order; the client polls for its own offers and renders the
-- confirm prompt. A DB row rather than a Cmd, so no wire-shape change and an
-- offer survives a client restart.
-- ============================================================

DEFINE TABLE assist_offer TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD answered_by       ON assist_offer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD connection_string ON assist_offer TYPE string PERMISSIONS FULL;
DEFINE FIELD checkin_notes     ON assist_offer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD created_at        ON assist_offer TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD customer_name     ON assist_offer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD device            ON assist_offer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD reason            ON assist_offer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD service_number    ON assist_offer TYPE none | string PERMISSIONS FULL;
DEFINE FIELD service_order     ON assist_offer TYPE none | record<service_order> PERMISSIONS FULL;
DEFINE FIELD status            ON assist_offer TYPE string DEFAULT 'offered'
    ASSERT $value IN ['offered', 'accepted', 'declined', 'expired'] PERMISSIONS FULL;

DEFINE INDEX assist_offer_conn_idx ON assist_offer FIELDS connection_string;

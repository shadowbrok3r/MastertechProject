-- ============================================================
-- TABLE: assist_message
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Two-way transcript between a technician in MasterTech and a zeroclaw agent.
-- The client writes `direction = 'in'` rows; the admin-agent forwards them to
-- the zeroclaw channel and writes the agent's replies back as `direction = 'out'`.
-- Bench clients never reach the agent host directly, so this table is the rail.

DEFINE TABLE assist_message TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE FIELD connection_string ON assist_message TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD created_at        ON assist_message TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD direction         ON assist_message TYPE string ASSERT $value IN ['in', 'out'] PERMISSIONS FULL;
DEFINE FIELD error             ON assist_message TYPE none | string   PERMISSIONS FULL;
-- Conversation room shared with zeroclaw: the service number when known, else
-- the connection string. The agent session key also carries the sender, so two
-- techs on one service still get separate conversations.
DEFINE FIELD room              ON assist_message TYPE string          PERMISSIONS FULL;
DEFINE FIELD sent_at           ON assist_message TYPE none | datetime PERMISSIONS FULL;
DEFINE FIELD service_number    ON assist_message TYPE none | string   PERMISSIONS FULL;
-- zeroclaw's own session key, resolved after the first reply.
DEFINE FIELD session_key       ON assist_message TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD status            ON assist_message TYPE string DEFAULT 'pending' ASSERT $value IN ['pending', 'sent', 'failed', 'delivered'] PERMISSIONS FULL;
DEFINE FIELD tech              ON assist_message TYPE none | string   PERMISSIONS FULL;
DEFINE FIELD text              ON assist_message TYPE string          PERMISSIONS FULL;
-- MasterTech chat thread the message belongs to.
DEFINE FIELD thread            ON assist_message TYPE string          PERMISSIONS FULL;

DEFINE INDEX assist_message_thread_idx ON assist_message FIELDS thread;
DEFINE INDEX assist_message_pending_idx ON assist_message FIELDS direction, status;

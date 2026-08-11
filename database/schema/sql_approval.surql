-- ============================================================
-- TABLE: sql_approval
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- Root-gated approval queue for SurrealQL mutations submitted through the
-- MCP bridge. The requesting process writes a pending row and polls it; a
-- Root operator decides from the admin console; the requester executes only
-- once status reads 'approved'. DDL never reaches this table — the MCP tool
-- rejects DEFINE/REMOVE/ALTER so surrealkit stays the only path to schema.

DEFINE TABLE sql_approval TYPE ANY SCHEMALESS CHANGEFEED 2d PERMISSIONS FULL;

DEFINE FIELD statement        ON sql_approval TYPE string                              PERMISSIONS FULL;
DEFINE FIELD reason           ON sql_approval TYPE string                              PERMISSIONS FULL;
DEFINE FIELD statement_kind   ON sql_approval TYPE string DEFAULT 'unknown'            PERMISSIONS FULL;
DEFINE FIELD target_table     ON sql_approval TYPE none | string                       PERMISSIONS FULL;
DEFINE FIELD impact_rows      ON sql_approval TYPE none | int                          PERMISSIONS FULL;
DEFINE FIELD impact_note      ON sql_approval TYPE none | string                       PERMISSIONS FULL;
DEFINE FIELD requested_by     ON sql_approval TYPE none | record<user>                 PERMISSIONS FULL;
DEFINE FIELD requested_label  ON sql_approval TYPE string DEFAULT ''                   PERMISSIONS FULL;
DEFINE FIELD origin_host      ON sql_approval TYPE string DEFAULT ''                   PERMISSIONS FULL;
DEFINE FIELD status           ON sql_approval TYPE string DEFAULT 'pending'            PERMISSIONS FULL;
DEFINE FIELD decided_by       ON sql_approval TYPE none | record<user>                 PERMISSIONS FULL;
DEFINE FIELD decided_at       ON sql_approval TYPE none | datetime                     PERMISSIONS FULL;
DEFINE FIELD deny_reason      ON sql_approval TYPE none | string                       PERMISSIONS FULL;
DEFINE FIELD result_summary   ON sql_approval TYPE none | string                       PERMISSIONS FULL;
DEFINE FIELD executed_at      ON sql_approval TYPE none | datetime                     PERMISSIONS FULL;
DEFINE FIELD created_at       ON sql_approval TYPE none | datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD expires_at       ON sql_approval TYPE none | datetime                     PERMISSIONS FULL;

DEFINE INDEX sql_approval_status_idx ON sql_approval FIELDS status;

-- Notifies every Root operator on request, so an approval reaches them the
-- same way an Admin notification does. Root is resolved from authorization
-- rather than a hardcoded id, so adding a Root user needs no schema change.
DEFINE EVENT sql_approval_requested ON sql_approval WHEN $event = 'CREATE' THEN {
    FOR $root IN (SELECT VALUE id FROM user WHERE authorization = 'Root' AND active = true) {
        CREATE notification CONTENT {
            notification_description: string::concat('SurrealQL approval requested by ', $value.requested_label, ' — ', $value.reason),
            notification_type: 'Approval',
            status: 'Unread',
            user: $root
        }
    }
};

-- Stamps the decision time whenever a Root operator resolves a request.
DEFINE EVENT sql_approval_decided ON sql_approval WHEN $event = 'UPDATE'
    AND $before.status = 'pending' AND $after.status != 'pending' THEN {
    UPDATE $value.id SET decided_at = time::now()
};

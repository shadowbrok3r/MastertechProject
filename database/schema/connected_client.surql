-- ============================================================
-- TABLE: connected_client          last changed: Tranche 1 (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE connected_client TYPE NORMAL SCHEMALESS CHANGEFEED 1w PERMISSIONS FULL;

DEFINE EVENT uid_event ON connected_client
    WHEN $event = 'CREATE'
    THEN {
        UPDATE $value.id SET created_at = time::now();
    };

DEFINE EVENT `update` ON connected_client
    WHEN $event = 'UPDATE'
    THEN {
        IF $this.assigned_user == $auth.id {
            IF $this.created_at == NULL OR $this.created_at == NONE {
                UPDATE $this.id SET created_at = time::now();
            };
            IF $before.connected == false AND $after.connected == true {
                UPDATE $this.id SET last_update = time::now();
                CREATE notification CONTENT {
                    notification_description: string::concat('Your Client (', $value.connection_string, ') Has Connected'),
                    notification_type: 'Client Connected',
                    status: 'Unread',
                    user: $value.assigned_user
                };
            };
        };
    };

DEFINE FIELD assigned_user    ON connected_client TYPE none | record<user>    DEFAULT $auth.id PERMISSIONS FULL;
DEFINE FIELD command_history  ON connected_client TYPE none | array<object>  PERMISSIONS FULL;
DEFINE FIELD command_history.* ON connected_client TYPE object               PERMISSIONS FULL;
DEFINE FIELD computer         ON connected_client TYPE none | record<computer> PERMISSIONS FULL;
DEFINE FIELD created_at       ON connected_client TYPE none | datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD customer         ON connected_client TYPE none | record<customer> PERMISSIONS FULL;
DEFINE FIELD customer_locked  ON connected_client TYPE bool DEFAULT false     PERMISSIONS FULL;
DEFINE FIELD friendly_name    ON connected_client TYPE none | string          PERMISSIONS FULL;
DEFINE FIELD last_update      ON connected_client TYPE none | datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD local_ip         ON connected_client TYPE none | string          PERMISSIONS FULL;
DEFINE FIELD tcp_port         ON connected_client TYPE none | number DEFAULT 9101 PERMISSIONS FULL;
-- Distinguishes a customer-machine client from a Rust-toolchain
-- `plugin_builder` worker. The admin/MCP `list_build_workers` tool
-- filters on this; the Web Console default view hides workers.
-- Rows without this field are treated as `machine` for back-compat.
DEFINE FIELD client_kind      ON connected_client TYPE string DEFAULT 'machine' ASSERT $value IN ['machine', 'build_worker'] PERMISSIONS FULL;

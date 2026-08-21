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

-- WHEN stays on plain field checks; the is_internal traversal costs a fetch, so it
-- runs in THEN. Terminates because the write makes customer NONE.
DEFINE EVENT internal_machine_client_has_no_customer ON connected_client
    WHEN $value.customer != NONE AND $value.computer != NONE
    THEN {
        IF $value.computer.is_internal == true {
            UPDATE $value.id SET customer = NONE;
        };
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

-- Guest/system connections (axum ingest) have a non-user or absent $auth;
-- only a real user auth may become the default assignee.
DEFINE FIELD assigned_user    ON connected_client TYPE none | record<user>    DEFAULT IF $auth.id != NONE AND record::tb($auth.id) == 'user' { $auth.id } PERMISSIONS FULL;
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
DEFINE FIELD client_kind      ON connected_client TYPE string DEFAULT 'machine' ASSERT $value IN ['machine', 'build_worker', 'qc_agent', 'uefi'] PERMISSIONS FULL;
-- Which OS the current session checked in from. A WinPE session derives its
-- record key from the offline install's hostname so it lands on the same row as
-- that machine's normal Windows check-in; this field is what still marks the
-- session as pre-boot. Rows without it are treated as `installed`.
DEFINE FIELD boot_environment ON connected_client TYPE string DEFAULT 'installed' ASSERT $value IN ['installed', 'winpe'] PERMISSIONS FULL;
-- Excludes a machine from unattended agent sweeps. Sweeps are opt-out, so the
-- absence of this field means eligible; DEFAULT does not backfill existing rows,
-- so readers must coalesce NONE to false.
DEFINE FIELD autopilot_opt_out ON connected_client TYPE bool DEFAULT false PERMISSIONS FULL;

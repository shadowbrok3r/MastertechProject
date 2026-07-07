-- ============================================================
-- TABLE: service_order             last changed: Tranche 1 (2026-05-16)
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE service_order TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE EVENT created ON service_order
    WHEN $event = 'CREATE'
    THEN {
        UPDATE $value.id SET created_at = time::now();
        IF $value.computer.id == NONE AND $value.customer != NONE {
            LET $cust = $value.customer;
            LET $pc = (SELECT VALUE id FROM computer WHERE customer == $cust)[0];
            IF $pc != NONE {
                UPDATE $value.id SET computer = $pc;
            };
        };
    };

DEFINE INDEX service_idx        ON service_order FIELDS service_number;
DEFINE INDEX service_number_idx ON service_order FIELDS service_number UNIQUE;

DEFINE FIELD checkin_notes       ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD checkin_rep         ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD computer            ON service_order TYPE none | record<computer> PERMISSIONS FULL;
DEFINE FIELD created_at          ON service_order TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD current_antivirus   ON service_order TYPE none | array<string>  PERMISSIONS FULL;
DEFINE FIELD current_antivirus.* ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD customer            ON service_order TYPE none | record<customer> PERMISSIONS FULL;
DEFINE FIELD doc_alias           ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD hardware_test_results ON service_order TYPE object              PERMISSIONS FULL;
DEFINE FIELD sales_rep           ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD service_number      ON service_order TYPE none | string DEFAULT '' PERMISSIONS FULL;
DEFINE FIELD tech                ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD terms               ON service_order TYPE string                PERMISSIONS FULL;
DEFINE FIELD ticket_total        ON service_order TYPE string                PERMISSIONS FULL;

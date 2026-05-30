-- ============================================================
-- TABLE: customer
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================

DEFINE TABLE customer TYPE ANY SCHEMALESS PERMISSIONS FULL;

DEFINE INDEX customer_code_idx  ON customer FIELDS cust_code UNIQUE;
DEFINE INDEX customer_email_idx ON customer FIELDS email     UNIQUE;

DEFINE FIELD computers       ON customer TYPE none | array<record<computer>> DEFAULT []   PERMISSIONS FULL;
DEFINE FIELD computers.*     ON customer TYPE record<computer>                            PERMISSIONS FULL;
DEFINE FIELD cust_code       ON customer TYPE string                                      PERMISSIONS FULL;
DEFINE FIELD email           ON customer TYPE string                                      PERMISSIONS FULL;
DEFINE FIELD li_amnt         ON customer TYPE string                                      PERMISSIONS FULL;
DEFINE FIELD name            ON customer TYPE string                                      PERMISSIONS FULL;
DEFINE FIELD num_inv         ON customer TYPE string                                      PERMISSIONS FULL;
DEFINE FIELD phone_number    ON customer TYPE string                                      PERMISSIONS FULL;
DEFINE FIELD phone_number_2  ON customer TYPE none | string                               PERMISSIONS FULL;
DEFINE FIELD services        ON customer TYPE none | array<record<service_order>>         PERMISSIONS FULL;
DEFINE FIELD services.*      ON customer TYPE record<service_order>                        PERMISSIONS FULL;

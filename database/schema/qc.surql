-- ============================================================
-- TABLE: qc
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- ============================================================
-- PrestaShop order mirror used by the QC tab. Field shapes mirror the
-- imported PrestaShop JSON (all strings), including the explicit `id` field.

DEFINE TABLE qc TYPE ANY SCHEMALESS CHANGEFEED 1w PERMISSIONS FULL;

DEFINE FIELD associations.order_rows.*.id                 ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD associations.order_rows.*.id_order_config    ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD associations.order_rows.*.product_id         ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD associations.order_rows.*.product_name       ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD associations.order_rows.*.product_price      ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD associations.order_rows.*.product_quantity   ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD associations.order_rows.*.product_reference  ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD current_state          ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD date_add               ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD date_upd               ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD delivery_date          ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id                     ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_address_delivery    ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_address_invoice     ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_customer            ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_employee_editing    ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_employee_sales_rep  ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_employee_split_rep  ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_order_everest       ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_order_parent        ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_order_type          ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD id_store               ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD invoice_date           ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD invoice_number         ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD order_type             ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD payment                ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD reference              ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD shipping_number        ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD task                   ON qc TYPE record<task> PERMISSIONS FULL;
DEFINE FIELD total_paid             ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD total_paid_tax_excl    ON qc TYPE string PERMISSIONS FULL;
DEFINE FIELD total_products_wt      ON qc TYPE string PERMISSIONS FULL;

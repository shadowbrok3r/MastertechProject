-- ============================================================
-- TABLE: customer_identity
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- Maps one customer across the PrestaShop and Shopify id spaces. Mastertech
-- couples `customer.cust_code` to the PrestaShop `id_customer`, so this table
-- keeps those integer keys valid while Shopify GIDs come into use.
-- ============================================================

DEFINE TABLE customer_identity TYPE ANY SCHEMAFULL PERMISSIONS FULL;

-- PrestaShop `id_customer`, which Mastertech also stores as `customer.cust_code`.
DEFINE FIELD ps_id       ON customer_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
-- `gid://shopify/Customer/123`.
DEFINE FIELD shopify_gid ON customer_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
DEFINE FIELD cust_code   ON customer_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
DEFINE FIELD email       ON customer_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
DEFINE FIELD backend     ON customer_identity TYPE string
    ASSERT $value IN ['prestashop', 'shopify'] PERMISSIONS FULL;
DEFINE FIELD created_at  ON customer_identity TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD updated_at  ON customer_identity TYPE datetime DEFAULT time::now() PERMISSIONS FULL;

-- Indexes come after the fields they cover: a file is applied in order, and an
-- index on a not-yet-defined field aborts the sync.
-- UNIQUE admits any number of NONE rows while still rejecting a duplicate real
-- id, which is why every optional key above defaults to NONE and never to ''.
DEFINE INDEX customer_identity_ps_idx    ON customer_identity FIELDS ps_id UNIQUE;
DEFINE INDEX customer_identity_gid_idx   ON customer_identity FIELDS shopify_gid UNIQUE;
DEFINE INDEX customer_identity_code_idx  ON customer_identity FIELDS cust_code;
DEFINE INDEX customer_identity_email_idx ON customer_identity FIELDS email;

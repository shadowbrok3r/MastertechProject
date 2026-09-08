-- ============================================================
-- TABLE: order_identity
-- Managed by surrealkit. Edit here, then `rollout plan`.
-- Maps one order across the PrestaShop and Shopify id spaces and records
-- which backend owns it. Written at intake; read by order routing so the
-- backend is looked up rather than guessed from the key's digit shape.
-- ============================================================

DEFINE TABLE order_identity TYPE ANY SCHEMAFULL PERMISSIONS FULL;

-- PrestaShop `id_order`. NONE for an order that never existed in PrestaShop.
DEFINE FIELD ps_id       ON order_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
-- `gid://shopify/Order/123`. NONE for a PrestaShop-only order.
DEFINE FIELD shopify_gid ON order_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
-- Shopify order number without the `#`, e.g. `1042`.
DEFINE FIELD shopify_order_number ON order_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
-- Everest document number, which routes to PrestaShop.
DEFINE FIELD everest_doc ON order_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
-- PrestaShop `reference` / Shopify `name`.
DEFINE FIELD reference   ON order_identity TYPE none | string DEFAULT NONE PERMISSIONS FULL;
DEFINE FIELD backend     ON order_identity TYPE string
    ASSERT $value IN ['prestashop', 'shopify'] PERMISSIONS FULL;
DEFINE FIELD created_at  ON order_identity TYPE datetime DEFAULT time::now() PERMISSIONS FULL;
DEFINE FIELD updated_at  ON order_identity TYPE datetime DEFAULT time::now() PERMISSIONS FULL;

-- Indexes come after the fields they cover: a file is applied in order, and an
-- index on a not-yet-defined field aborts the sync.
-- UNIQUE admits any number of NONE rows while still rejecting a duplicate real
-- id, which is why every optional key above defaults to NONE and never to ''.
DEFINE INDEX order_identity_ps_idx        ON order_identity FIELDS ps_id UNIQUE;
DEFINE INDEX order_identity_gid_idx       ON order_identity FIELDS shopify_gid UNIQUE;
DEFINE INDEX order_identity_number_idx    ON order_identity FIELDS shopify_order_number;
DEFINE INDEX order_identity_everest_idx   ON order_identity FIELDS everest_doc;
DEFINE INDEX order_identity_reference_idx ON order_identity FIELDS reference;

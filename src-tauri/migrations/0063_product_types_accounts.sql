-- P2 Wave 1: product types / groups + default-account mapping
-- Purely additive — no existing table or column is modified.

-- 1. Add product_type: canonical type drives default GL accounts.
ALTER TABLE products ADD COLUMN product_type TEXT NOT NULL DEFAULT 'marfa';

-- 2. Add optional group FK (nullable — groups are created later by the user).
ALTER TABLE products ADD COLUMN product_group_id TEXT;

-- 3. Product groups — company-scoped named groupings.
CREATE TABLE product_groups (
    id         TEXT    PRIMARY KEY NOT NULL,
    company_id TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_product_groups_company ON product_groups(company_id);

-- 4. Account mapping — per-company OVERRIDES only (empty = use code defaults).
--    The 5 canonical rows are never seeded here; the Rust layer returns the
--    code-default when no override row exists for a (company_id, product_type) pair.
CREATE TABLE account_mapping (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    product_type    TEXT    NOT NULL,
    stock_account   TEXT,
    expense_account TEXT,
    income_account  TEXT,
    uses_stock      INTEGER NOT NULL DEFAULT 1,
    retail_capable  INTEGER NOT NULL DEFAULT 0,
    updated_at      INTEGER NOT NULL,
    UNIQUE(company_id, product_type)
);
CREATE INDEX idx_account_mapping_company ON account_mapping(company_id);

-- 5. Backfill: products that were is_service=1 become 'serviciu'; everything else stays 'marfa'.
UPDATE products SET product_type = 'serviciu' WHERE is_service = 1;

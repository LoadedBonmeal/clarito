-- Migration 0013: Products / catalog table.
-- Stores reusable product/service entries per company (company-scoped).
-- Monetary and quantity values stored as TEXT to match the app's Decimal-as-TEXT convention.

CREATE TABLE IF NOT EXISTS products (
    id           TEXT    PRIMARY KEY,
    company_id   TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name         TEXT    NOT NULL,
    unit         TEXT    NOT NULL DEFAULT 'buc',
    unit_price   TEXT    NOT NULL DEFAULT '0.00',
    vat_rate     TEXT    NOT NULL DEFAULT '19',
    vat_category TEXT    NOT NULL DEFAULT 'S',
    code         TEXT,
    stock_qty    TEXT,
    active       INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_products_company ON products(company_id);

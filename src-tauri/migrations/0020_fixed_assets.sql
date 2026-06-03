-- Phase 6b: fixed assets (Assets SAF-T section / MasterFiles)
-- Straight-line depreciation calculator lives in db/assets.rs.

CREATE TABLE IF NOT EXISTS fixed_assets (
    id                  TEXT    NOT NULL PRIMARY KEY,
    company_id          TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    asset_code          TEXT    NOT NULL,               -- unique per company
    account_id          TEXT    NOT NULL DEFAULT '213', -- 21x fixed-asset GL account
    description         TEXT    NOT NULL,
    valuation_class     TEXT    NOT NULL DEFAULT 'Corporala', -- ValuationClass for SAF-T
    supplier_id         TEXT    NOT NULL DEFAULT '0',   -- canonical partner ID
    supplier_name       TEXT    NOT NULL DEFAULT '',
    date_of_acquisition TEXT    NOT NULL,               -- YYYY-MM-DD
    start_up_date       TEXT    NOT NULL,               -- YYYY-MM-DD commissioning date
    acquisition_cost    TEXT    NOT NULL DEFAULT '0.00',
    life_months         INTEGER NOT NULL DEFAULT 60,    -- useful life in months
    depreciation_method TEXT    NOT NULL DEFAULT 'liniara',
    depreciation_pct    TEXT    NOT NULL DEFAULT '0.00',-- annual % (if given; else computed)
    disposal_date       TEXT,                           -- YYYY-MM-DD when scrapped/sold
    active              INTEGER NOT NULL DEFAULT 1,
    created_at          INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at          INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, asset_code)
);

CREATE TABLE IF NOT EXISTS asset_transactions (
    id               TEXT    NOT NULL PRIMARY KEY,
    company_id       TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    asset_id         TEXT    NOT NULL REFERENCES fixed_assets(id) ON DELETE CASCADE,
    transaction_code TEXT    NOT NULL,               -- unique ref per asset
    transaction_type TEXT    NOT NULL DEFAULT '10',  -- AssetTransactionType numeric code
    transaction_date TEXT    NOT NULL,               -- YYYY-MM-DD
    description      TEXT    NOT NULL DEFAULT '',
    gl_transaction_id TEXT,                          -- cross-ref to GL
    acq_prod_cost    TEXT    NOT NULL DEFAULT '0.00',
    book_value       TEXT    NOT NULL DEFAULT '0.00',
    amount           TEXT    NOT NULL DEFAULT '0.00',
    created_at       INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_fixed_assets_company
    ON fixed_assets(company_id);

CREATE INDEX IF NOT EXISTS idx_asset_transactions_company_date
    ON asset_transactions(company_id, transaction_date);

CREATE INDEX IF NOT EXISTS idx_asset_transactions_asset
    ON asset_transactions(asset_id);
